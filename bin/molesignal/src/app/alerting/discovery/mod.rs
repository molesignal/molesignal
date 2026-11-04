// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 自动发现的后台任务：每个组织每 [`ROUND_EVERY`] 找一遍 APM、RUM、日志（[`logs`]）和资源指标
//! （[`metrics`]）的问题，结果并进 `discovery_findings`（见 [`crate::domain::alerting::discovery`]）。
//! 组织按 id 分给各 alert 节点；结束满 30 天的发现由主节点删掉。
//!
//! 一边读不出来（APM 或 RUM 不可用、查询出错）时，这一轮不动那一边进行中的发现，免得它们被当成
//! 没看到而结束；日志按流算、指标按种类算。
//!
//! 规则的数据来源是自动发现时，评估器从这里读进行中的发现（[`findings_result`]）：每条一行，
//! 按发现拆分，值是严重度的档位。

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use serde_json::json;
use tokio::task::JoinHandle;

use crate::{
    app::{apm::ApmQueryService, cluster::WorkOwnership},
    domain::{
        alerting::discovery::{
            DISCOVERY_DIMENSIONS, DISCOVERY_VALUE_COLUMN, DiscoveryQuery, DiscoveryRepository,
            Finding, FindingKind, FindingSource, KEEP_ENDED_DAYS, merge, severity_rank,
        },
        iam::OrganizationRepository,
        query::{QueryResult, StreamHint},
        stream::StreamType,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

mod apm;
mod logs;
mod metrics;
mod rum;

pub use logs::LogDiscovery;
pub use metrics::MetricDiscovery;
pub use rum::RumDiscovery;

/// 多久找一遍。
const ROUND_EVERY: Duration = Duration::from_secs(5 * 60);

/// 启动后多久找第一遍：等 APM 汇总先跑一轮。
const FIRST_ROUND_AFTER: Duration = Duration::from_secs(60);

/// 归属指标里这类工作的名字。
const WORK: &str = "discovery";

const DAY_MICROS: i64 = 86_400_000_000;

pub struct DiscoveryWorker {
    findings: Arc<dyn DiscoveryRepository>,
    orgs: Arc<dyn OrganizationRepository>,
    apm: Option<Arc<ApmQueryService>>,
    rum: Option<Arc<RumDiscovery>>,
    logs: Option<Arc<LogDiscovery>>,
    metrics: Option<Arc<MetricDiscovery>>,
    ownership: Arc<WorkOwnership>,
}

impl DiscoveryWorker {
    pub fn new(
        findings: Arc<dyn DiscoveryRepository>,
        orgs: Arc<dyn OrganizationRepository>,
        apm: Option<Arc<ApmQueryService>>,
        rum: Option<Arc<RumDiscovery>>,
        ownership: Arc<WorkOwnership>,
    ) -> Self {
        Self {
            findings,
            orgs,
            apm,
            rum,
            logs: None,
            metrics: None,
            ownership,
        }
    }

    /// 也找日志和资源指标的问题。
    pub fn with_signals(mut self, logs: Arc<LogDiscovery>, metrics: Arc<MetricDiscovery>) -> Self {
        self.logs = Some(logs);
        self.metrics = Some(metrics);
        self
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(self.run())
    }

    #[tracing::instrument(
        name = "worker.discovery",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "discovery")
    )]
    async fn run(self) {
        tokio::time::sleep(FIRST_ROUND_AFTER).await;
        let mut tick = tokio::time::interval(ROUND_EVERY);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tick.tick().await;
            self.round(TimestampMicros::now()).await;
        }
    }

    /// 找一遍本节点负责的组织。
    pub async fn round(&self, now: TimestampMicros) {
        let orgs = match self.orgs.list().await {
            Ok(orgs) => orgs,
            Err(error) => {
                tracing::warn!(%error, "organizations could not be read for discovery");
                return;
            }
        };

        let mut owned = 0;

        for org in orgs {
            if !self.ownership.owns(org.id.as_str()).await {
                continue;
            }

            owned += 1;

            if let Err(error) = self.round_for(&org.id, now).await {
                tracing::warn!(org_id = %org.id, %error, "discovery round failed");
            }
        }

        self.ownership.record_owned(WORK, owned);

        if self.ownership.is_primary().await {
            let before = TimestampMicros(now.0 - KEEP_ENDED_DAYS * DAY_MICROS);

            if let Err(error) = self.findings.delete_ended_before(before).await {
                tracing::warn!(%error, "ended discovery findings could not be removed");
            }
        }
    }

    async fn round_for(&self, org_id: &Id, now: TimestampMicros) -> Result<()> {
        let mut detections = Vec::new();
        // 这一轮读出来了的：只有它们进行中的发现才跟着这一轮走。
        let mut read = Read::default();

        if let Some(apm) = &self.apm {
            match apm::detect(apm, org_id, now).await {
                Ok(found) => {
                    detections.extend(found);
                    read.sources.insert(FindingSource::Apm);
                }
                Err(error) => {
                    tracing::warn!(%org_id, %error, "APM could not be read for discovery");
                }
            }
        }

        if let Some(rum) = &self.rum {
            match rum.detect(org_id, now).await {
                Ok(found) => {
                    detections.extend(found);
                    read.sources.insert(FindingSource::Rum);
                }
                Err(error) => {
                    tracing::warn!(%org_id, %error, "RUM could not be read for discovery");
                }
            }
        }

        if let Some(logs) = &self.logs {
            match logs.detect(org_id, now).await {
                Ok(scan) => {
                    detections.extend(scan.detections);
                    read.log_streams = scan.scanned;
                }
                Err(error) => {
                    tracing::warn!(%org_id, %error, "log streams could not be read for discovery");
                }
            }
        }

        if let Some(metrics) = &self.metrics {
            match metrics.detect(org_id, now).await {
                Ok(scan) => {
                    detections.extend(scan.detections);
                    read.metric_kinds = scan.scanned;
                }
                Err(error) => {
                    tracing::warn!(%org_id, %error, "metrics could not be read for discovery");
                }
            }
        }

        let open: Vec<Finding> = self
            .findings
            .list_open(org_id)
            .await?
            .into_iter()
            .filter(|finding| read.covers(finding))
            .collect();

        let written = merge(org_id, open, detections, now);
        self.findings.save(&written).await
    }
}

/// 这一轮读到了什么：APM、RUM 整边算，日志按流，指标按种类。
#[derive(Default)]
struct Read {
    sources: HashSet<FindingSource>,
    log_streams: HashSet<String>,
    metric_kinds: HashSet<FindingKind>,
}

impl Read {
    fn covers(&self, finding: &Finding) -> bool {
        match finding.kind.source() {
            FindingSource::Apm | FindingSource::Rum => {
                self.sources.contains(&finding.kind.source())
            }
            FindingSource::Logs => finding
                .detail
                .get("stream")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|stream| self.log_streams.contains(stream)),
            FindingSource::Metrics => self.metric_kinds.contains(&finding.kind),
        }
    }
}

/// 自动发现规则看到的「查询结果」：命中的每条进行中的发现一行，按发现、种类、对象拆分，值是
/// 严重度的档位。
pub fn findings_result(findings: &[Finding], query: &DiscoveryQuery, took_ms: u64) -> QueryResult {
    let rows: Vec<Vec<serde_json::Value>> = findings
        .iter()
        .filter(|finding| finding.is_open() && query.matches(finding))
        .map(|finding| {
            vec![
                json!(finding.id.0),
                json!(finding.kind.as_str()),
                json!(finding.subject),
                json!(severity_rank(finding.severity)),
                json!(finding.title),
            ]
        })
        .collect();

    let mut columns: Vec<String> = DISCOVERY_DIMENSIONS
        .iter()
        .map(|column| column.to_string())
        .collect();
    columns.push(DISCOVERY_VALUE_COLUMN.into());
    columns.push("title".into());

    QueryResult {
        columns,
        scanned_rows: rows.len() as u64,
        rows,
        took_ms,
        federation: None,
    }
}

/// 自动发现规则在评估器要数据流的地方顶上的那个：它不跑查询。
pub fn discovery_stream_hint() -> StreamHint {
    StreamHint {
        name: "discovery".into(),
        stream_type: StreamType::LOGS,
    }
}

/// 进行中的发现按种类数一数：给洞察和 Agent 用。
pub fn open_by_kind(findings: &[Finding]) -> HashMap<FindingKind, usize> {
    let mut counts = HashMap::new();

    for finding in findings.iter().filter(|finding| finding.is_open()) {
        *counts.entry(finding.kind).or_default() += 1;
    }

    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::alerting::{discovery::Detection, incident::Severity};

    fn finding(kind: FindingKind, severity: Severity) -> Finding {
        merge(
            &Id::from_string("org"),
            Vec::new(),
            vec![Detection {
                kind,
                key: format!("{}|x", kind.as_str()),
                severity,
                subject: "default/checkout".into(),
                environment: None,
                metric: None,
                title: "t".into(),
                value: None,
                baseline: None,
                started_at: None,
                detail: json!({}),
            }],
            TimestampMicros(1),
        )
        .remove(0)
    }

    #[test]
    fn a_discovery_rule_sees_one_row_per_matching_open_finding() {
        let findings = vec![
            finding(FindingKind::ApmAnomaly, Severity::Critical),
            finding(FindingKind::RumNewIssue, Severity::Warning),
        ];

        let all = findings_result(&findings, &DiscoveryQuery::default(), 0);
        assert_eq!(all.rows.len(), 2);
        assert_eq!(all.columns[3], DISCOVERY_VALUE_COLUMN);
        assert_eq!(all.rows[0][3], json!(4.0));

        let apm_only = findings_result(
            &findings,
            &DiscoveryQuery {
                kinds: vec![FindingKind::ApmAnomaly],
                ..DiscoveryQuery::default()
            },
            0,
        );
        assert_eq!(apm_only.rows.len(), 1);
        assert_eq!(open_by_kind(&findings).len(), 2);
    }
}
