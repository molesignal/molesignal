// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 自动发现：不用建规则就能找到的问题。每个组织每几分钟找一遍，结果存成「发现」：
//!
//! - APM：服务的错误率、P95、流量相对前一段和上周同段突变（[`FindingKind::ApmAnomaly`]），新版本
//!   比旧版本差（[`FindingKind::ApmRegression`]）；
//! - RUM：新出现的错误 issue（[`FindingKind::RumNewIssue`]），解决后又出现的
//!   （[`FindingKind::RumRegressedIssue`]）。
//!
//! 同一个问题（种类、对象、指标拼成的 `key`）同时只有一条进行中的发现：再看到就更新现值和最近一次
//! 看到的时间；连续 [`ENDS_AFTER_MISSED_ROUNDS`] 轮没看到就算结束。结束满 [`KEEP_ENDED_DAYS`] 天
//! 删掉。发现只进「自动发现」列表；要通知，建一条数据来源是自动发现的规则（[`DiscoveryQuery`]），
//! 每条命中的发现各开一个事件，发现结束事件就恢复。

use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    incident::Severity,
    rule::{AlertRule, AlertRuleKind, ComparisonOp, SeverityThreshold},
};
use crate::{
    domain::query::QueryLanguage,
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// 连续几轮没看到就算结束。
pub const ENDS_AFTER_MISSED_ROUNDS: u32 = 3;

/// 结束的发现留几天。
pub const KEEP_ENDED_DAYS: i64 = 30;

/// 规则里最多列几个对象、几个环境。
pub const MAX_DISCOVERY_SUBJECTS: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    /// 服务的错误率、P95 或流量突变。
    ApmAnomaly,
    /// 新版本比旧版本差。
    ApmRegression,
    /// 新出现的前端错误 issue。
    RumNewIssue,
    /// 解决后又出现的前端错误 issue。
    RumRegressedIssue,
    /// 一个日志流（或其中一个服务）的错误级日志突然变多。
    LogErrorSurge,
    /// 此前一天没出现过的错误日志（按模板归一）。
    LogNewError,
    /// 磁盘、文件系统或内存快满了。
    MetricSaturation,
    /// 主机 CPU 使用率突变。
    MetricShift,
    /// 容器在重启。
    MetricRestarts,
}

/// 发现从哪一边来：这一边这一轮读不出来时，它进行中的发现不动。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FindingSource {
    Apm,
    Rum,
    Logs,
    Metrics,
}

impl FindingKind {
    pub const ALL: [Self; 9] = [
        Self::ApmAnomaly,
        Self::ApmRegression,
        Self::RumNewIssue,
        Self::RumRegressedIssue,
        Self::LogErrorSurge,
        Self::LogNewError,
        Self::MetricSaturation,
        Self::MetricShift,
        Self::MetricRestarts,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ApmAnomaly => "apm_anomaly",
            Self::ApmRegression => "apm_regression",
            Self::RumNewIssue => "rum_new_issue",
            Self::RumRegressedIssue => "rum_regressed_issue",
            Self::LogErrorSurge => "log_error_surge",
            Self::LogNewError => "log_new_error",
            Self::MetricSaturation => "metric_saturation",
            Self::MetricShift => "metric_shift",
            Self::MetricRestarts => "metric_restarts",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }

    pub const fn source(self) -> FindingSource {
        match self {
            Self::ApmAnomaly | Self::ApmRegression => FindingSource::Apm,
            Self::RumNewIssue | Self::RumRegressedIssue => FindingSource::Rum,
            Self::LogErrorSurge | Self::LogNewError => FindingSource::Logs,
            Self::MetricSaturation | Self::MetricShift | Self::MetricRestarts => {
                FindingSource::Metrics
            }
        }
    }
}

/// 一条发现。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub id: Id,
    pub org_id: Id,
    pub kind: FindingKind,
    /// 认出同一个问题用的：种类、对象和指标拼成。
    pub key: String,
    pub severity: Severity,
    /// APM 服务（`命名空间/服务名`）、RUM 应用 id、日志里的服务（没有服务字段时是日志流）、主机或
    /// 容器。
    pub subject: String,
    #[serde(default)]
    pub environment: Option<String>,
    /// APM 突变的指标（`error_rate`、`p95`、`throughput`），或指标发现看的指标名。
    #[serde(default)]
    pub metric: Option<String>,
    pub title: String,
    #[serde(default)]
    pub value: Option<f64>,
    /// 拿来比的值：前一段的，或旧版本的。
    #[serde(default)]
    pub baseline: Option<f64>,
    pub started_at: TimestampMicros,
    pub last_seen_at: TimestampMicros,
    #[serde(default)]
    pub ended_at: Option<TimestampMicros>,
    /// 连续几轮没看到。
    #[serde(default)]
    pub missed_rounds: u32,
    /// 附近的变更、版本、issue 等，原样给详情看。
    #[serde(default)]
    pub detail: Value,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

impl Finding {
    pub fn is_open(&self) -> bool {
        self.ended_at.is_none()
    }
}

/// 一轮找到的一个问题。
#[derive(Debug, Clone, PartialEq)]
pub struct Detection {
    pub kind: FindingKind,
    pub key: String,
    pub severity: Severity,
    pub subject: String,
    pub environment: Option<String>,
    pub metric: Option<String>,
    pub title: String,
    pub value: Option<f64>,
    pub baseline: Option<f64>,
    /// 问题从什么时候开始；不知道时按这一轮。
    pub started_at: Option<TimestampMicros>,
    pub detail: Value,
}

/// 把一轮找到的并进进行中的发现，返回要写的：新开的、再看到的、没看到的（够轮数就结束）。
pub fn merge(
    org_id: &Id,
    open: Vec<Finding>,
    detections: Vec<Detection>,
    now: TimestampMicros,
) -> Vec<Finding> {
    let mut open: HashMap<String, Finding> = open
        .into_iter()
        .filter(Finding::is_open)
        .map(|finding| (finding.key.clone(), finding))
        .collect();

    let mut written = Vec::new();

    for detection in detections {
        let finding = match open.remove(&detection.key) {
            Some(mut finding) => {
                finding.severity = finding.severity.max(detection.severity);
                finding.title = detection.title;
                finding.value = detection.value;
                finding.baseline = detection.baseline;
                finding.last_seen_at = now;
                finding.missed_rounds = 0;
                finding.detail = detection.detail;
                finding.updated_at = now;
                finding
            }
            None => Finding {
                id: Id::new(),
                org_id: org_id.clone(),
                kind: detection.kind,
                key: detection.key,
                severity: detection.severity,
                subject: detection.subject,
                environment: detection.environment,
                metric: detection.metric,
                title: detection.title,
                value: detection.value,
                baseline: detection.baseline,
                started_at: detection.started_at.unwrap_or(now).min(now),
                last_seen_at: now,
                ended_at: None,
                missed_rounds: 0,
                detail: detection.detail,
                created_at: now,
                updated_at: now,
            },
        };

        written.push(finding);
    }

    for mut finding in open.into_values() {
        finding.missed_rounds += 1;
        finding.updated_at = now;

        if finding.missed_rounds >= ENDS_AFTER_MISSED_ROUNDS {
            finding.ended_at = Some(now);
        }

        written.push(finding);
    }

    written
}

/// 规则的数据来源是自动发现时，挑哪些发现：都不给就是全部。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryQuery {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<FindingKind>,
    /// APM 服务（`命名空间/服务名` 或服务名）或 RUM 应用 id。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subjects: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub environments: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_severity: Option<Severity>,
}

impl DiscoveryQuery {
    pub fn validate(&self) -> Result<()> {
        if self.subjects.len() > MAX_DISCOVERY_SUBJECTS
            || self.environments.len() > MAX_DISCOVERY_SUBJECTS
        {
            return Err(crate::shared::Error::invalid(format!(
                "a discovery rule names at most {MAX_DISCOVERY_SUBJECTS} services and environments"
            )));
        }

        Ok(())
    }

    /// 这条发现归这条规则管。服务名不带命名空间时按服务名比。
    pub fn matches(&self, finding: &Finding) -> bool {
        let subject = |wanted: &String| {
            *wanted == finding.subject
                || finding
                    .subject
                    .rsplit_once('/')
                    .is_some_and(|(_, name)| name == wanted)
        };

        (self.kinds.is_empty() || self.kinds.contains(&finding.kind))
            && (self.subjects.is_empty() || self.subjects.iter().any(subject))
            && (self.environments.is_empty()
                || finding
                    .environment
                    .as_ref()
                    .is_some_and(|environment| self.environments.contains(environment)))
            && self.min_severity.is_none_or(|bar| finding.severity >= bar)
    }
}

/// 自动发现规则的结果按这几列拆分：每条发现一组，各开一个事件；它们也成了事件的标签。
pub const DISCOVERY_DIMENSIONS: [&str; 3] =
    ["discovery.finding", "discovery.kind", "discovery.subject"];

/// 结果里的值：发现的严重度的档位（提示 1、警告 2、错误 3、严重 4）。
pub const DISCOVERY_VALUE_COLUMN: &str = "severity_rank";

/// 自动发现规则的语句：不跑查询，只是一个占位。
pub const DISCOVERY_STATEMENT: &str = "discovery";

/// 自动发现规则多久评估一次。
pub const DISCOVERY_PERIOD_SECS: u32 = 60;

pub fn severity_rank(severity: Severity) -> f64 {
    match severity {
        Severity::Info => 1.0,
        Severity::Warning => 2.0,
        Severity::Error => 3.0,
        Severity::Critical => 4.0,
    }
}

/// 数据来源是自动发现的规则只有一种形状，保存时统一成它：按发现拆分，值是发现的严重度档位，
/// 四档阈值各对一档，所以事件的严重度就是发现的严重度；发现结束（这一组没值）事件就恢复。
pub fn shape_rule(rule: &mut AlertRule) -> Result<()> {
    let Some(discovery) = &rule.query.discovery else {
        return Ok(());
    };

    discovery.validate()?;

    rule.kind = AlertRuleKind::Scheduled;
    rule.query.language = QueryLanguage::Sql;
    rule.query.statement = DISCOVERY_STATEMENT.into();
    rule.query.period_secs = DISCOVERY_PERIOD_SECS;
    rule.query.stream = None;
    rule.query.apm = None;
    rule.query.rum = None;
    rule.query.composite = None;
    rule.query.forecast = None;
    rule.query.outlier = None;
    rule.query.heartbeat = None;
    rule.query.delay_secs = None;
    rule.query.home_cluster = None;
    rule.query.group_by = DISCOVERY_DIMENSIONS
        .iter()
        .map(|column| column.to_string())
        .collect();
    rule.anomaly_params = None;
    rule.severity = None;

    rule.trigger.operator = ComparisonOp::Gte;
    rule.trigger.threshold = 1.0;
    rule.trigger.for_periods = 1;
    rule.trigger.no_data = None;
    rule.trigger.recovery = None;

    rule.thresholds = [
        Severity::Info,
        Severity::Warning,
        Severity::Error,
        Severity::Critical,
    ]
    .into_iter()
    .map(|severity| SeverityThreshold {
        severity,
        operator: ComparisonOp::Gte,
        threshold: severity_rank(severity),
        for_periods: 1,
    })
    .collect();

    Ok(())
}

/// 列发现时的条件。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FindingFilter {
    /// `Some(true)` 只要进行中的，`Some(false)` 只要结束的。
    pub open: Option<bool>,
    pub kind: Option<FindingKind>,
    /// 最近一次看到在这之后。
    pub since: Option<TimestampMicros>,
    pub limit: usize,
}

#[async_trait]
pub trait DiscoveryRepository: Send + Sync {
    /// 组织进行中的发现。
    async fn list_open(&self, org_id: &Id) -> Result<Vec<Finding>>;

    /// 按条件列，最近看到的在前。
    async fn list(&self, org_id: &Id, filter: &FindingFilter) -> Result<Vec<Finding>>;

    async fn get(&self, org_id: &Id, id: &Id) -> Result<Option<Finding>>;

    /// 按 id 写入或覆盖。
    async fn save(&self, findings: &[Finding]) -> Result<()>;

    /// 删掉在这之前结束的，返回删了几条。
    async fn delete_ended_before(&self, before: TimestampMicros) -> Result<u64>;
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn detection(key: &str, severity: Severity) -> Detection {
        Detection {
            kind: FindingKind::ApmAnomaly,
            key: key.into(),
            severity,
            subject: "default/checkout".into(),
            environment: Some("prod".into()),
            metric: Some("error_rate".into()),
            title: "Errors of checkout up".into(),
            value: Some(0.12),
            baseline: Some(0.01),
            started_at: Some(TimestampMicros(50)),
            detail: json!({}),
        }
    }

    #[test]
    fn a_problem_seen_again_stays_one_finding_and_ends_after_missed_rounds() {
        let org = Id::from_string("org-1");

        let opened = merge(
            &org,
            Vec::new(),
            vec![detection("k", Severity::Warning)],
            TimestampMicros(100),
        );
        assert_eq!(opened.len(), 1);
        assert_eq!(opened[0].started_at, TimestampMicros(50));
        assert!(opened[0].is_open());

        let seen = merge(
            &org,
            opened.clone(),
            vec![detection("k", Severity::Critical)],
            TimestampMicros(200),
        );
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].id, opened[0].id);
        assert_eq!(seen[0].severity, Severity::Critical);
        assert_eq!(seen[0].last_seen_at, TimestampMicros(200));

        let mut current = seen;

        for round in 1..=ENDS_AFTER_MISSED_ROUNDS {
            current = merge(
                &org,
                current,
                Vec::new(),
                TimestampMicros(200 + i64::from(round)),
            );
            assert_eq!(current[0].missed_rounds, round);
        }

        assert_eq!(current[0].ended_at, Some(TimestampMicros(203)));
        // 结束的不再跟着。
        assert!(merge(&org, current, Vec::new(), TimestampMicros(300)).is_empty());
    }

    #[test]
    fn a_rule_picks_findings_by_kind_service_environment_and_severity() {
        let org = Id::from_string("org-1");
        let finding = merge(
            &org,
            Vec::new(),
            vec![detection("k", Severity::Warning)],
            TimestampMicros(1),
        )
        .remove(0);

        assert!(DiscoveryQuery::default().matches(&finding));
        assert!(
            DiscoveryQuery {
                kinds: vec![FindingKind::ApmAnomaly],
                subjects: vec!["checkout".into()],
                environments: vec!["prod".into()],
                min_severity: Some(Severity::Warning),
            }
            .matches(&finding)
        );
        assert!(
            !DiscoveryQuery {
                kinds: vec![FindingKind::RumNewIssue],
                ..DiscoveryQuery::default()
            }
            .matches(&finding)
        );
        assert!(
            !DiscoveryQuery {
                min_severity: Some(Severity::Critical),
                ..DiscoveryQuery::default()
            }
            .matches(&finding)
        );
        assert!(
            !DiscoveryQuery {
                environments: vec!["staging".into()],
                ..DiscoveryQuery::default()
            }
            .matches(&finding)
        );
        assert_eq!(
            FindingKind::parse("rum_new_issue"),
            Some(FindingKind::RumNewIssue)
        );
    }
}
