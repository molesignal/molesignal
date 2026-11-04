// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! APM 那一半：用洞察的检测找最近半小时里服务的错误率、P95、流量突变和变差的新版本。性能建议
//! （慢依赖、N+1、慢语句、热点接口）不算问题，不收。

use std::sync::Arc;

use serde_json::json;

use crate::{
    app::apm::{ApmQueryRequest, ApmQueryService, Insight, InsightKind, InsightSeverity},
    domain::alerting::{
        discovery::{Detection, FindingKind},
        incident::Severity,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// 看最近多久。
const LOOKBACK_MICROS: i64 = 30 * 60 * 1_000_000;

pub(super) async fn detect(
    apm: &Arc<ApmQueryService>,
    org_id: &Id,
    now: TimestampMicros,
) -> Result<Vec<Detection>> {
    let request = ApmQueryRequest {
        from: now.0 - LOOKBACK_MICROS,
        to: now.0,
        ..ApmQueryRequest::default()
    };

    let context = apm.context(org_id.clone(), request, &["request_count"], "request_count")?;
    let insights = apm.insights(&context).await?.insights;

    Ok(insights.into_iter().filter_map(detection).collect())
}

fn detection(insight: Insight) -> Option<Detection> {
    let kind = match insight.kind {
        InsightKind::Anomaly => FindingKind::ApmAnomaly,
        InsightKind::Regression => FindingKind::ApmRegression,
        _ => return None,
    };

    let severity = match insight.severity {
        InsightSeverity::Critical => Severity::Critical,
        InsightSeverity::Warning => Severity::Warning,
        InsightSeverity::Info => return None,
    };

    let service = &insight.service;
    let subject = format!("{}/{}", service.namespace, service.name);
    let metric = insight.metric.unwrap_or("requests");

    let (key, title, baseline) = match kind {
        FindingKind::ApmRegression => {
            let version = insight.version.clone().unwrap_or_default();

            (
                format!(
                    "{}|{subject}|{}|{version}",
                    kind.as_str(),
                    service.environment
                ),
                format!(
                    "{} {version} does worse than the version before",
                    service.name
                ),
                insight.previous,
            )
        }
        _ => (
            format!(
                "{}|{subject}|{}|{metric}",
                kind.as_str(),
                service.environment
            ),
            format!("{} of {} moved", metric_name(metric), service.name),
            insight.previous.or(insight.last_week),
        ),
    };

    Some(Detection {
        kind,
        key,
        severity,
        environment: (!service.environment.is_empty()).then(|| service.environment.clone()),
        subject,
        metric: insight.metric.map(str::to_string),
        title,
        value: Some(insight.value),
        baseline,
        started_at: insight.since,
        detail: json!({
            "previous": insight.previous,
            "lastWeek": insight.last_week,
            "version": insight.version,
            "previousVersion": insight.previous_version,
            "changes": insight.changes,
        }),
    })
}

fn metric_name(metric: &str) -> &'static str {
    match metric {
        "error_rate" => "Error rate",
        "p95" => "P95 latency",
        "throughput" => "Traffic",
        _ => "Requests",
    }
}
