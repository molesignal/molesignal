// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! RUM 那一半：各应用最近半小时第一次出现的错误 issue，和解决后又出现的，直接读 issue 跟踪已经
//! 算好的结果。忽略了的 issue 不收。

use std::sync::Arc;

use serde_json::json;

use crate::{
    domain::{
        alerting::{
            discovery::{Detection, FindingKind},
            incident::Severity,
        },
        rum::{RumApplicationRepository, RumIssueMoment, RumIssueRepository, RumIssueStatus},
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// 看最近多久。
const LOOKBACK_MICROS: i64 = 30 * 60 * 1_000_000;

/// 一个应用一轮最多收几条。
const MOST_PER_APPLICATION: usize = 50;

pub struct RumDiscovery {
    applications: Arc<dyn RumApplicationRepository>,
    issues: Arc<dyn RumIssueRepository>,
}

impl RumDiscovery {
    pub fn new(
        applications: Arc<dyn RumApplicationRepository>,
        issues: Arc<dyn RumIssueRepository>,
    ) -> Self {
        Self {
            applications,
            issues,
        }
    }

    pub(super) async fn detect(&self, org_id: &Id, now: TimestampMicros) -> Result<Vec<Detection>> {
        let from = TimestampMicros(now.0 - LOOKBACK_MICROS);
        let mut detections = Vec::new();

        for application in self.applications.list(org_id).await? {
            let app = application.id.as_str();

            let first = self
                .issues
                .first_seen_between(org_id, app, None, from, now, MOST_PER_APPLICATION)
                .await?;
            let back = self
                .issues
                .regressed_between(org_id, app, None, from, now, MOST_PER_APPLICATION)
                .await?;

            detections.extend(
                self.described(
                    org_id,
                    app,
                    &application.name,
                    FindingKind::RumNewIssue,
                    first,
                )
                .await?,
            );
            detections.extend(
                self.described(
                    org_id,
                    app,
                    &application.name,
                    FindingKind::RumRegressedIssue,
                    back,
                )
                .await?,
            );
        }

        Ok(detections)
    }

    async fn described(
        &self,
        org_id: &Id,
        app: &str,
        name: &str,
        kind: FindingKind,
        moments: Vec<RumIssueMoment>,
    ) -> Result<Vec<Detection>> {
        if moments.is_empty() {
            return Ok(Vec::new());
        }

        let ids: Vec<String> = moments
            .iter()
            .map(|moment| moment.issue_id.clone())
            .collect();
        let issues = self.issues.get_many(org_id, &ids).await?;

        Ok(moments
            .into_iter()
            .filter_map(|moment| {
                let issue = issues.iter().find(|issue| issue.id == moment.issue_id)?;

                if matches!(
                    issue.status,
                    RumIssueStatus::Ignored | RumIssueStatus::Resolved
                ) {
                    return None;
                }

                let message = if issue.message.is_empty() {
                    issue.error_type.clone()
                } else {
                    issue.message.clone()
                };

                Some(Detection {
                    kind,
                    key: format!("{}|{app}|{}", kind.as_str(), issue.id),
                    // 回归的问题比新问题更糟：修过的又坏了。
                    severity: match kind {
                        FindingKind::RumRegressedIssue => Severity::Error,
                        _ => Severity::Warning,
                    },
                    subject: app.to_string(),
                    environment: issue.regressed_environment.clone(),
                    metric: None,
                    title: format!("{name}: {message}"),
                    value: None,
                    baseline: None,
                    started_at: Some(moment.at),
                    detail: json!({
                        "issueId": issue.id,
                        "application": name,
                        "errorType": issue.error_type,
                        "location": issue.location,
                        "version": issue.regressed_version.as_ref().or(issue.last_version.as_ref()),
                    }),
                })
            })
            .collect())
    }
}
