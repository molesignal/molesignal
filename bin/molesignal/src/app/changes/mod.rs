// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Every change of a stretch of time in one list: what pipelines and tools
//! reported (deployments, configuration, feature flags, scaling,
//! infrastructure), annotations people put on a time range, the versions APM
//! first saw in spans, and edits of alert rules. Each kind is read only for a
//! reader allowed to read it. Charts mark the list on their time axis.
//!
//! Reported changes come in through [`report`]: from pipelines (`POST /apm/deployments`), from
//! webhooks read by [`intake`], and from `to_change` pipeline steps.

pub mod intake;
mod merge;
mod report;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use domain::{
    alerting::repositories::AlertRuleRepository,
    apm::{ApmCatalogRepository, DeploymentQuery},
};
use kernel::{Result, ids::Id, time::TimeRange};
pub use merge::{ChangeEntry, Sources, merge};
use postgres::repositories::annotations::{AnnotationFilter, AnnotationRepository};
pub use report::{ChangeRecorder, ReportedChange, change_event};
use serde::{Deserialize, Serialize};

use super::apm::{ApmQueryRequest, ApmQueryService};

/// The most entries a list answers.
pub const MOST_CHANGES: usize = 500;

/// What a change is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeFeedKind {
    Deployment,
    Configuration,
    FeatureFlag,
    Scaling,
    Infrastructure,
    Annotation,
    /// A version APM first saw in spans.
    Version,
    /// An edit of an alert rule.
    Rule,
}

impl ChangeFeedKind {
    pub const ALL: [Self; 8] = [
        Self::Deployment,
        Self::Configuration,
        Self::FeatureFlag,
        Self::Scaling,
        Self::Infrastructure,
        Self::Annotation,
        Self::Version,
        Self::Rule,
    ];

    /// Any one of these lets a reader read changes of this kind.
    pub fn permissions(self) -> &'static [&'static str] {
        match self {
            Self::Deployment
            | Self::Configuration
            | Self::FeatureFlag
            | Self::Scaling
            | Self::Infrastructure => &["apm.deployments.read", "sys.telemetry.read"],
            Self::Annotation => &["dashboards.annotations.read"],
            Self::Version => &["apm.services.read", "sys.telemetry.read"],
            Self::Rule => &["alerts.rules.read"],
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Deployment => "deployment",
            Self::Configuration => "configuration",
            Self::FeatureFlag => "feature_flag",
            Self::Scaling => "scaling",
            Self::Infrastructure => "infrastructure",
            Self::Annotation => "annotation",
            Self::Version => "version",
            Self::Rule => "rule",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }

    /// Reported by a pipeline or a tool.
    pub fn reported(self) -> bool {
        !matches!(self, Self::Annotation | Self::Version | Self::Rule)
    }
}

/// Which changes a list keeps.
#[derive(Debug, Clone)]
pub struct ChangeFeedQuery {
    pub range: TimeRange,
    /// Changes of these services, and those of no service; every one's when empty.
    pub services: Vec<String>,
    pub namespace: Option<String>,
    pub environment: Option<String>,
    /// These kinds; every kind the reader may read when empty.
    pub kinds: Vec<ChangeFeedKind>,
    /// Annotations of this dashboard or this log stream, and those of neither.
    pub dashboard_id: Option<String>,
    pub stream: Option<String>,
    pub limit: usize,
}

/// Where changes are read from.
#[derive(Clone)]
pub struct ChangeFeed {
    pub catalog: Arc<dyn ApmCatalogRepository>,
    pub annotations: Arc<dyn AnnotationRepository>,
    pub apm: Arc<ApmQueryService>,
    pub rules: Arc<dyn AlertRuleRepository>,
}

impl ChangeFeed {
    /// The changes `query` keeps, the latest first, of the kinds `holds` lets the reader read.
    pub async fn read(
        &self,
        org_id: &Id,
        query: &ChangeFeedQuery,
        holds: impl Fn(&str) -> bool,
    ) -> Result<Vec<ChangeEntry>> {
        let wanted = ChangeFeedKind::ALL
            .into_iter()
            .filter(|kind| query.kinds.is_empty() || query.kinds.contains(kind))
            .filter(|kind| {
                kind.permissions()
                    .iter()
                    .any(|permission| holds(permission))
            })
            .collect::<Vec<_>>();

        let wants = |kind: ChangeFeedKind| wanted.contains(&kind);

        let reported = async {
            if !wanted.iter().any(|kind| kind.reported()) {
                return Ok(Vec::new());
            }

            self.catalog
                .list_deployments(
                    org_id,
                    &DeploymentQuery {
                        namespace: query.namespace.clone(),
                        service: (query.services.len() == 1).then(|| query.services[0].clone()),
                        environment: query.environment.clone(),
                        from: Some(query.range.start),
                        to: Some(query.range.end),
                        limit: query.limit,
                    },
                )
                .await
        };

        let annotations = async {
            if !wants(ChangeFeedKind::Annotation) {
                return Ok(Vec::new());
            }

            self.annotations
                .list(
                    org_id,
                    AnnotationFilter {
                        from_micros: Some(query.range.start.0),
                        to_micros: Some(query.range.end.0),
                        ..AnnotationFilter::default()
                    },
                )
                .await
        };

        let versions = async {
            if !wants(ChangeFeedKind::Version) {
                return Ok(Vec::new());
            }

            let context = self.apm.context(
                org_id.clone(),
                ApmQueryRequest {
                    from: query.range.start.0,
                    to: query.range.end.0,
                    namespace: query.namespace.clone(),
                    environment: query.environment.clone(),
                    ..ApmQueryRequest::default()
                },
                &["name"],
                "name",
            )?;

            Ok::<_, kernel::Error>(self.apm.inventory(&context).await?.versions)
        };

        let rules = async {
            if !wants(ChangeFeedKind::Rule) {
                return Ok(Vec::new());
            }

            self.rules.list(org_id).await
        };

        let (reported, annotations, versions, rules) =
            tokio::try_join!(reported, annotations, versions, rules)?;

        Ok(merge(
            Sources {
                reported,
                annotations,
                versions,
                rules,
            },
            query,
            &wanted,
        ))
    }
}
