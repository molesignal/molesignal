// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

pub use super::rule_conditions::{
    MAX_CONDITION_PERIODS, NoDataAction, NoDataCondition, RecoveryCondition, recovery_operator,
    validate_conditions,
};
use super::{
    anomaly::AnomalyParams, composite::CompositeQuery, discovery::DiscoveryQuery,
    forecast::ForecastParams, heartbeat::HeartbeatParams, incident::Severity,
    outlier::OutlierParams, priority::Priority, urgency::Urgency,
};
use crate::{
    domain::{
        apm::{ApmAlertMetric, ApmAlertQuery},
        query::{QueryLanguage, StreamHint},
        rum::RumAlertQuery,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 规则求值类型。不同 kind 分派到不同评估管线：
/// - [`Scheduled`](AlertRuleKind::Scheduled)：周期性阈值评估（默认，向后兼容）。
/// - [`RealTime`](AlertRuleKind::RealTime)：写入路径上的匹配器逐条求值，统计最近
///   `period_secs` 秒内满足条件的记录数并与触发条件比较（语句形态见 `SELECT count(*) FROM
///   <stream> [WHERE ...]`）。匹配器不支持的语句仍由周期评估器评估。
/// - [`Anomaly`](AlertRuleKind::Anomaly)：基线对比，携带 [`AnomalyParams`]。
/// - [`Composite`](AlertRuleKind::Composite)：组合别的规则（`query.composite`），没有自己的查询。
/// - [`Forecast`](AlertRuleKind::Forecast)：拿过去的序列外推，预计越线就触发（`query.forecast`）。
/// - [`Outlier`](AlertRuleKind::Outlier)：按列拆分，各组相互比较（`query.outlier`）。
/// - [`Heartbeat`](AlertRuleKind::Heartbeat)：等打卡，没有查询（`query.heartbeat`）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertRuleKind {
    #[default]
    Scheduled,
    RealTime,
    Anomaly,
    Composite,
    Forecast,
    Outlier,
    Heartbeat,
}

impl AlertRuleKind {
    /// 持久化到 `alert_rules.kind`（VARCHAR(16)）用的稳定字符串。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Scheduled => "scheduled",
            Self::RealTime => "real_time",
            Self::Anomaly => "anomaly",
            Self::Composite => "composite",
            Self::Forecast => "forecast",
            Self::Outlier => "outlier",
            Self::Heartbeat => "heartbeat",
        }
    }

    /// 没有阈值可比的种类：判定由它自己的算法给出，不带多档阈值和回差线。
    pub const fn judges_itself(self) -> bool {
        matches!(
            self,
            Self::Anomaly | Self::Composite | Self::Forecast | Self::Outlier | Self::Heartbeat
        )
    }

    /// 不跑查询的种类：组合规则看别的规则，心跳规则看打卡。
    pub const fn without_query(self) -> bool {
        matches!(self, Self::Composite | Self::Heartbeat)
    }

    /// 从持久化字符串解析；未知值回退 [`Scheduled`](AlertRuleKind::Scheduled)。
    pub fn parse(s: &str) -> Self {
        match s {
            "anomaly" => Self::Anomaly,
            "real_time" | "realtime" => Self::RealTime,
            "composite" => Self::Composite,
            "forecast" => Self::Forecast,
            "outlier" => Self::Outlier,
            "heartbeat" => Self::Heartbeat,
            _ => Self::Scheduled,
        }
    }
}

/// 告警规则。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertRule {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    /// 求值类型；缺省（历史 JSON）反序列化为 [`Scheduled`](AlertRuleKind::Scheduled)。
    #[serde(default)]
    pub kind: AlertRuleKind,
    pub query: AlertQuery,
    pub trigger: AlertTrigger,
    /// 仅 `kind = Anomaly` 携带；其它 kind 为 `None`。
    #[serde(default)]
    pub anomaly_params: Option<AnomalyParams>,
    /// 多档严重度阈值；空 = 走单档 `trigger`（向后兼容）。逐档评估，取最高命中档定 severity。
    #[serde(default)]
    pub thresholds: Vec<SeverityThreshold>,
    /// 单档规则的显式兜底严重度；`None` = 由评估器推导（determine_severity）。
    #[serde(default)]
    pub severity: Option<Severity>,
    /// 关联的升级策略 ID（由 [`super::escalation::EscalationPolicy`] 定义）
    pub escalation_policy_id: Id,
    pub labels: std::collections::BTreeMap<String, String>,
    pub annotations: std::collections::BTreeMap<String, String>,
    /// 上一次评估完成的时间（None = 从未评估过）。
    pub last_eval_at: Option<TimestampMicros>,
    /// 上一次评估完成后规则所处状态；evaluator 重启会丢，但 fingerprint 去重兜底。
    #[serde(default)]
    pub last_state: RuleState,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

/// 评估状态机：用于 evaluator 内存里跟踪 `for_periods` 计数 + 触发去抖。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "consecutive")]
pub enum RuleState {
    /// 最近一次评估未命中阈值。
    #[default]
    Healthy,
    /// 已连续命中 N 次（< for_periods），还在去抖窗口。
    Pending(u32),
    /// 已触发，并创建/更新过 incident。
    Firing,
    /// 最近一次评估没有值，按规则保持原状态（`no_data` 为 `keep` 或 `alert`）。
    NoData,
    /// 已触发，但要开的事件被告警流水线丢掉了：规则命中了，却没有事件开着。哪条流水线、哪一步丢的，
    /// 记在那条流水线上。
    Dropped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertQuery {
    pub language: QueryLanguage,
    pub statement: String,
    /// 评估窗口（秒）。
    pub period_secs: u32,
    /// 命中查询的 stream（表）名 + 类型。当前查询引擎要求显式指定，
    /// evaluator 据此把 `QueryRequest.stream` 填出来。
    /// 历史持久化的 JSON 可能没有该字段：反序列化为 None，
    /// evaluator 看到 None 会跳过该规则并打一条 warn，不再调底层引擎。
    #[serde(default)]
    pub stream: Option<StreamHint>,
    /// An APM figure watched in place of the statement: a rule with one reads
    /// APM's aggregates and needs no stream; `statement` then only names it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apm: Option<ApmAlertQuery>,
    /// A RUM figure watched in place of the statement: like an APM figure it is read from the
    /// RUM data the pages show, and the rule names no stream.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rum: Option<RumAlertQuery>,
    /// 按查询结果里的这些列拆分：每组取值（如每台主机、每个服务）各自比较阈值、各开各的事件，组的
    /// 值是该行第一个数值列。空着时整条规则一个事件，取结果首行首列的值。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub group_by: Vec<String>,
    /// 组合规则：组合哪些规则、怎么组合。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composite: Option<CompositeQuery>,
    /// 预测规则：回看多久、提前量和外推方式。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forecast: Option<ForecastParams>,
    /// 离群规则：容差和看哪一侧。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outlier: Option<OutlierParams>,
    /// 心跳规则：多久打一次卡、宽限多久。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heartbeat: Option<HeartbeatParams>,
    /// 评估延迟（秒，0 到 [`MAX_EVALUATION_DELAY_SECS`]）：每轮看 `[now − 延迟 − 窗口, now − 延迟]`，
    /// 等晚到或还没刷写的数据。空着时按部署自动定，见 [`EvaluationDelay`]。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay_secs: Option<u32>,
    /// 数据来源是自动发现：每条命中的发现各开一个事件，发现结束事件就恢复（见
    /// [`super::discovery`]）。保存时规则统一成固定的形状。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discovery: Option<DiscoveryQuery>,
    /// 实时规则的汇总集群（联邦里的集群 id）：空着时每个集群各算各的；设了时只在这个集群开事件，
    /// 它把联邦里映射了这个组织的各集群的计数加起来，别的集群只计数。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home_cluster: Option<String>,
}

/// 评估延迟最长多久：1 小时。
pub const MAX_EVALUATION_DELAY_SECS: u32 = 3600;

/// 规则没设评估延迟时用多少：单机读得到自己缓冲里的数据，不用等；分角色集群里查询只读已刷写的
/// 文件，用实例设置里的默认值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvaluationDelay {
    /// 评估的节点自己也接收写入、查询读得到它的缓冲。
    pub reads_own_buffer: bool,
    /// 实例设置里的默认值（秒）。
    pub cluster_default_secs: u32,
}

impl EvaluationDelay {
    /// 这条规则的评估延迟（秒）：规则自己设的，否则按部署定。
    pub fn for_query(&self, query: &AlertQuery) -> u32 {
        query.delay_secs.unwrap_or(if self.reads_own_buffer {
            0
        } else {
            self.cluster_default_secs
        })
    }
}

/// 一条规则最多按几列拆分。
pub const MAX_GROUP_BY_COLUMNS: usize = 8;

/// 一条拆分的规则一轮评估最多看多少组；更多的组这一轮不判断。
pub const MAX_ALERT_GROUPS: usize = 100;

/// 拆分用的列名：去掉首尾空白，不能为空、重复或太多。
fn normalize_group_by(columns: Vec<String>) -> Result<Vec<String>> {
    if columns.len() > MAX_GROUP_BY_COLUMNS {
        return Err(Error::invalid(format!(
            "query.group_by names at most {MAX_GROUP_BY_COLUMNS} columns"
        )));
    }

    let mut normalized: Vec<String> = Vec::with_capacity(columns.len());

    for column in columns {
        let column = column.trim().to_string();

        if column.is_empty() || column.len() > 128 {
            return Err(Error::invalid(
                "query.group_by columns must be names of 1 to 128 characters",
            ));
        }

        if normalized.contains(&column) {
            return Err(Error::invalid(format!(
                "query.group_by names `{column}` twice"
            )));
        }

        normalized.push(column);
    }

    Ok(normalized)
}

impl AlertQuery {
    /// 规则按哪一种检查和保存：数据来源是自动发现的规则是定时规则，不管请求写的是哪一种。
    pub fn rule_kind(&self, requested: AlertRuleKind) -> AlertRuleKind {
        if self.discovery.is_some() {
            AlertRuleKind::Scheduled
        } else {
            requested
        }
    }

    /// Checks what a rule of `kind` reads before it is saved: a statement over
    /// a stream, an APM figure or a RUM figure. An APM or a RUM rule is
    /// evaluated on a schedule, against thresholds or against the same window
    /// on the days before, and names no stream; an empty statement is filled
    /// with what it watches.
    pub fn normalize(&mut self, kind: AlertRuleKind) -> Result<()> {
        // 数据来源是自动发现的规则不读流也不跑查询：保存时整条规则统一成它的形状。
        if let Some(discovery) = &self.discovery {
            return discovery.validate();
        }

        self.group_by = normalize_group_by(std::mem::take(&mut self.group_by))?;
        self.keep_params_of(kind);
        self.home_cluster = self
            .home_cluster
            .take()
            .map(|home| home.trim().to_string())
            .filter(|home| !home.is_empty());

        if self
            .delay_secs
            .is_some_and(|delay| delay > MAX_EVALUATION_DELAY_SECS)
        {
            return Err(Error::invalid(format!(
                "delay_secs is at most {MAX_EVALUATION_DELAY_SECS}"
            )));
        }

        if kind.without_query() {
            return self.normalize_without_query(kind);
        }

        if let Some(forecast) = &self.forecast {
            forecast.validate()?;
        } else if kind == AlertRuleKind::Forecast {
            return Err(Error::invalid(
                "a forecast rule needs forecast: { lookback_days, lead_secs, model }",
            ));
        }

        if let Some(outlier) = &self.outlier {
            outlier.validate()?;

            if self.group_by.is_empty() {
                return Err(Error::invalid(
                    "an outlier rule compares groups: split it by at least one column",
                ));
            }
        } else if kind == AlertRuleKind::Outlier {
            return Err(Error::invalid(
                "an outlier rule needs outlier: { tolerance, direction }",
            ));
        }

        if !self.group_by.is_empty() {
            if self.apm.is_some() || self.rum.is_some() {
                return Err(Error::invalid(
                    "an APM or a RUM rule watches one scope and is not split by columns",
                ));
            }

            if !matches!(kind, AlertRuleKind::Scheduled | AlertRuleKind::Outlier) {
                return Err(Error::invalid(
                    "only scheduled rules compared against thresholds and outlier rules are \
                     split by columns",
                ));
            }
        }

        if self.apm.is_some() && self.rum.is_some() {
            return Err(Error::invalid(
                "a rule watches an APM figure or a RUM figure, not both",
            ));
        }

        if let Some(rum) = &self.rum {
            if kind == AlertRuleKind::RealTime {
                return Err(Error::invalid("a RUM rule is evaluated on a schedule"));
            }

            if kind == AlertRuleKind::Forecast && !rum.metric.supports_anomaly() {
                return Err(Error::invalid(
                    "new and regressed issues are watched against thresholds",
                ));
            }

            if kind == AlertRuleKind::Anomaly && !rum.metric.supports_anomaly() {
                return Err(Error::invalid(
                    "new and regressed issues are watched against thresholds",
                ));
            }

            rum.validate(self.period_secs)?;

            if self.statement.trim().is_empty() {
                self.statement = rum.describe();
            }

            self.stream = None;
            return Ok(());
        }

        if let Some(apm) = &self.apm {
            if kind == AlertRuleKind::RealTime {
                return Err(Error::invalid("an APM rule is evaluated on a schedule"));
            }

            if kind == AlertRuleKind::Anomaly
                && matches!(
                    apm.metric,
                    ApmAlertMetric::SloBurnRate | ApmAlertMetric::NewErrorGroups
                )
            {
                return Err(Error::invalid(
                    "an SLO's burn rate and new error groups are watched against thresholds",
                ));
            }

            apm.validate(self.period_secs)?;

            if self.statement.trim().is_empty() {
                self.statement = apm.describe();
            }

            self.stream = None;
            return Ok(());
        }

        if self.statement.trim().is_empty() {
            return Err(Error::invalid("query.statement cannot be empty"));
        }

        let Some(stream) = self.stream.as_ref() else {
            return Err(Error::invalid(
                "query.stream is required; must specify { name, stream_type }",
            ));
        };

        if stream.name.trim().is_empty() {
            return Err(Error::invalid("query.stream.name cannot be empty"));
        }

        Ok(())
    }

    /// 只留下 `kind` 用得上的参数，换过种类的规则不带着上一种的。
    fn keep_params_of(&mut self, kind: AlertRuleKind) {
        if kind != AlertRuleKind::RealTime {
            self.home_cluster = None;
        }

        if kind != AlertRuleKind::Composite {
            self.composite = None;
        }

        if kind != AlertRuleKind::Forecast {
            self.forecast = None;
        }

        if kind != AlertRuleKind::Outlier {
            self.outlier = None;
        }

        if kind != AlertRuleKind::Heartbeat {
            self.heartbeat = None;
        }
    }

    /// 组合与心跳规则：不跑查询，语句只写它看的是什么。
    fn normalize_without_query(&mut self, kind: AlertRuleKind) -> Result<()> {
        if !self.group_by.is_empty() || self.apm.is_some() || self.rum.is_some() {
            return Err(Error::invalid(
                "a composite or heartbeat rule runs no query: it is not split by columns and \
                 watches no APM or RUM figure",
            ));
        }

        self.statement = match kind {
            AlertRuleKind::Composite => {
                let composite = self.composite.as_mut().ok_or_else(|| {
                    Error::invalid("a composite rule needs composite: { expression, rules }")
                })?;

                composite.validate(None)?;
                composite.expression.clone()
            }
            _ => {
                let heartbeat = self.heartbeat.as_ref().ok_or_else(|| {
                    Error::invalid("a heartbeat rule needs heartbeat: { period_secs, grace_secs }")
                })?;

                heartbeat.validate()?;
                "heartbeat".to_string()
            }
        };

        self.stream = None;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertTrigger {
    pub operator: ComparisonOp,
    pub threshold: f64,
    /// 满足条件持续多少个评估周期才真正触发，防抖。
    pub for_periods: u32,
    /// 触发后静默多久不再重复触发（秒）。
    pub silence_secs: u32,
    /// 没有值时怎么算；不填时阈值规则当作恢复，异常规则保持原状态。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_data: Option<NoDataCondition>,
    /// 怎样才算恢复；不填时一轮不满足触发条件就恢复。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<RecoveryCondition>,
    /// 开出的事件的优先级；不填时不设（告警流水线还可以设）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    /// 开出的事件的紧急度；不填时按事件走的升级策略定。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub urgency: Option<Urgency>,
}

/// 多档阈值的一档：值满足 `operator` 与 `threshold` 比较、且连续 `for_periods` 个周期命中时，
/// 该档触发并把 incident 严重度映射为 `severity`。一条规则的多档逐档独立评估、取最高命中档。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeverityThreshold {
    pub severity: Severity,
    pub operator: ComparisonOp,
    pub threshold: f64,
    pub for_periods: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonOp {
    Gt,
    Gte,
    Lt,
    Lte,
    Eq,
    Neq,
}

impl ComparisonOp {
    /// `value` 与 `threshold` 比较是否成立。
    pub fn holds(self, value: f64, threshold: f64) -> bool {
        match self {
            Self::Gt => value > threshold,
            Self::Gte => value >= threshold,
            Self::Lt => value < threshold,
            Self::Lte => value <= threshold,
            Self::Eq => (value - threshold).abs() < f64::EPSILON,
            Self::Neq => (value - threshold).abs() >= f64::EPSILON,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        apm::ApmAlertMetric,
        rum::{RumAlertMetric, RumAlertQuery},
        stream::StreamType,
    };

    fn query(apm: Option<ApmAlertQuery>) -> AlertQuery {
        AlertQuery {
            language: QueryLanguage::Sql,
            statement: String::new(),
            period_secs: 300,
            stream: Some(StreamHint {
                name: "app".into(),
                stream_type: StreamType::LOGS,
            }),
            apm,
            rum: None,
            group_by: Vec::new(),
            composite: None,
            forecast: None,
            outlier: None,
            heartbeat: None,
            delay_secs: None,
            discovery: None,
            home_cluster: None,
        }
    }

    fn rum(metric: RumAlertMetric) -> RumAlertQuery {
        RumAlertQuery {
            application_id: "shop-web".into(),
            environment: Some("prod".into()),
            page: None,
            version: None,
            metric,
            min_samples: None,
            custom_name: None,
            statistic: None,
            search: None,
        }
    }

    #[test]
    fn a_discovery_rule_is_scheduled_and_names_no_stream() {
        let mut found = AlertQuery {
            stream: None,
            statement: String::new(),
            discovery: Some(DiscoveryQuery::default()),
            ..query(None)
        };

        assert_eq!(
            found.rule_kind(AlertRuleKind::Anomaly),
            AlertRuleKind::Scheduled
        );
        assert!(found.normalize(AlertRuleKind::Scheduled).is_ok());
        assert_eq!(
            query(None).rule_kind(AlertRuleKind::Anomaly),
            AlertRuleKind::Anomaly
        );
    }

    #[test]
    fn a_rum_rule_names_what_it_watches_and_no_stream() {
        let mut watched = AlertQuery {
            rum: Some(rum(RumAlertMetric::LcpP75)),
            ..query(None)
        };

        watched.normalize(AlertRuleKind::Scheduled).unwrap();

        assert_eq!(watched.statement, "rum:lcp_p75 shop-web");
        assert!(watched.stream.is_none());
        assert!(watched.normalize(AlertRuleKind::RealTime).is_err());
        // Against the days before: a figure that moves with traffic, never a count of new issues.
        assert!(watched.normalize(AlertRuleKind::Anomaly).is_ok());

        let new_issues = AlertQuery {
            rum: Some(rum(RumAlertMetric::NewIssues)),
            ..query(None)
        };

        assert!(
            new_issues
                .clone()
                .normalize(AlertRuleKind::Scheduled)
                .is_ok()
        );
        assert!(
            new_issues
                .clone()
                .normalize(AlertRuleKind::Anomaly)
                .is_err()
        );
    }

    #[test]
    fn a_rule_watches_one_kind_of_figure() {
        let mut both = AlertQuery {
            apm: Some(apm(ApmAlertMetric::P95)),
            rum: Some(rum(RumAlertMetric::LcpP75)),
            ..query(None)
        };

        assert!(both.normalize(AlertRuleKind::Scheduled).is_err());
    }

    #[test]
    fn a_rum_rule_is_checked_for_the_window_it_is_evaluated_over() {
        let mut quick = AlertQuery {
            period_secs: 10,
            rum: Some(rum(RumAlertMetric::ErrorRate)),
            ..query(None)
        };

        assert!(quick.normalize(AlertRuleKind::Scheduled).is_err());
    }

    fn apm(metric: ApmAlertMetric) -> ApmAlertQuery {
        ApmAlertQuery {
            namespace: None,
            service: "checkout".into(),
            environment: None,
            transaction: None,
            metric,
            slo_id: None,
            short_window_secs: None,
        }
    }

    #[test]
    fn an_apm_rule_names_what_it_watches_and_no_stream() {
        let mut watched = query(Some(apm(ApmAlertMetric::P95)));

        watched.normalize(AlertRuleKind::Scheduled).unwrap();

        assert_eq!(watched.statement, "apm:p95 checkout");
        assert!(watched.stream.is_none());
        assert!(watched.normalize(AlertRuleKind::RealTime).is_err());
        // Against the days before: a figure that moves with traffic, never a count of new groups.
        assert!(watched.normalize(AlertRuleKind::Anomaly).is_ok());
        assert!(
            query(Some(apm(ApmAlertMetric::NewErrorGroups)))
                .normalize(AlertRuleKind::Anomaly)
                .is_err()
        );
        assert!(
            query(Some(apm(ApmAlertMetric::SloBurnRate)))
                .normalize(AlertRuleKind::Scheduled)
                .is_err()
        );
    }

    #[test]
    fn a_rule_is_split_by_the_columns_it_names_once_each() {
        let mut split = AlertQuery {
            statement: "SELECT host, count(*) FROM app GROUP BY host".into(),
            group_by: vec![" host ".into(), "service".into()],
            ..query(None)
        };

        split.normalize(AlertRuleKind::Scheduled).unwrap();
        assert_eq!(split.group_by, vec!["host", "service"]);

        let mut twice = AlertQuery {
            group_by: vec!["host".into(), "host".into()],
            ..split.clone()
        };
        assert!(twice.normalize(AlertRuleKind::Scheduled).is_err());

        // 异常与实时规则不拆分；APM 与 RUM 规则本来就只看一个范围。
        assert!(split.clone().normalize(AlertRuleKind::Anomaly).is_err());
        assert!(split.clone().normalize(AlertRuleKind::RealTime).is_err());

        let mut apm_split = AlertQuery {
            group_by: vec!["host".into()],
            ..query(Some(apm(ApmAlertMetric::P95)))
        };
        assert!(apm_split.normalize(AlertRuleKind::Scheduled).is_err());
    }

    #[test]
    fn a_statement_rule_needs_a_statement_and_a_stream() {
        let mut empty = query(None);
        assert!(empty.normalize(AlertRuleKind::Scheduled).is_err());

        let mut streamless = AlertQuery {
            statement: "SELECT 1".into(),
            stream: None,
            ..query(None)
        };
        assert!(streamless.normalize(AlertRuleKind::Scheduled).is_err());

        let mut full = AlertQuery {
            statement: "SELECT 1".into(),
            ..query(None)
        };
        assert!(full.normalize(AlertRuleKind::Scheduled).is_ok());
        assert!(full.stream.is_some());
    }

    fn trigger(operator: ComparisonOp, threshold: f64) -> AlertTrigger {
        AlertTrigger {
            operator,
            threshold,
            for_periods: 1,
            silence_secs: 0,
            no_data: None,
            recovery: None,
            priority: None,
            urgency: None,
        }
    }

    fn line(threshold: f64) -> Option<RecoveryCondition> {
        Some(RecoveryCondition {
            threshold: Some(threshold),
            for_periods: 0,
        })
    }

    #[test]
    fn a_recovery_line_stands_on_the_healthy_side_of_every_threshold() {
        let scheduled = AlertRuleKind::Scheduled;

        let mut above = trigger(ComparisonOp::Gt, 90.0);
        above.recovery = line(80.0);
        assert!(validate_conditions(scheduled, &above, &[]).is_ok());

        above.recovery = line(95.0);
        assert!(validate_conditions(scheduled, &above, &[]).is_err());

        let mut below = trigger(ComparisonOp::Lt, 10.0);
        below.recovery = line(20.0);
        assert!(validate_conditions(scheduled, &below, &[]).is_ok());

        let mut equal = trigger(ComparisonOp::Eq, 1.0);
        equal.recovery = line(1.0);
        assert!(validate_conditions(scheduled, &equal, &[]).is_err());

        // With tiers the line is checked against each of them, and they must fire on one side.
        let tier = |operator, threshold| SeverityThreshold {
            severity: Severity::Warning,
            operator,
            threshold,
            for_periods: 1,
        };
        let mut tiered = trigger(ComparisonOp::Gt, 0.0);
        tiered.recovery = line(70.0);
        let tiers = [tier(ComparisonOp::Gte, 80.0), tier(ComparisonOp::Gt, 95.0)];
        assert!(validate_conditions(scheduled, &tiered, &tiers).is_ok());
        let both_sides = [tier(ComparisonOp::Gt, 80.0), tier(ComparisonOp::Lt, 5.0)];
        assert!(validate_conditions(scheduled, &tiered, &both_sides).is_err());

        above.recovery = line(80.0);
        assert!(validate_conditions(AlertRuleKind::Anomaly, &above, &[]).is_err());
    }

    #[test]
    fn a_real_time_rule_takes_no_conditions_and_rounds_are_capped() {
        let mut realtime = trigger(ComparisonOp::Gt, 5.0);
        realtime.no_data = Some(NoDataCondition {
            action: NoDataAction::Keep,
            for_periods: 0,
        });
        assert!(validate_conditions(AlertRuleKind::RealTime, &realtime, &[]).is_err());

        let mut long = trigger(ComparisonOp::Gt, 5.0);
        long.recovery = Some(RecoveryCondition {
            threshold: None,
            for_periods: MAX_CONDITION_PERIODS + 1,
        });
        assert!(validate_conditions(AlertRuleKind::Scheduled, &long, &[]).is_err());
    }

    #[test]
    fn a_missing_value_resolves_a_threshold_rule_and_keeps_an_anomaly_rule_unless_told() {
        let mut told = trigger(ComparisonOp::Gt, 1.0);
        assert_eq!(
            told.no_data_action(AlertRuleKind::Scheduled),
            NoDataAction::Resolve
        );
        assert_eq!(
            told.no_data_action(AlertRuleKind::Anomaly),
            NoDataAction::Keep
        );
        assert_eq!(told.no_data_periods(), 1);
        assert_eq!(told.recovery_periods(), 1);

        told.no_data = Some(NoDataCondition {
            action: NoDataAction::Alert,
            for_periods: 0,
        });
        assert_eq!(
            told.no_data_action(AlertRuleKind::Anomaly),
            NoDataAction::Alert
        );
        assert_eq!(told.no_data_periods(), 1);
    }

    #[test]
    fn the_recovery_line_is_compared_as_the_first_tier_to_fire() {
        let tier = |operator, threshold| SeverityThreshold {
            severity: Severity::Warning,
            operator,
            threshold,
            for_periods: 1,
        };
        let base = trigger(ComparisonOp::Gt, 0.0);

        let above = [tier(ComparisonOp::Gt, 95.0), tier(ComparisonOp::Gte, 80.0)];
        assert!(matches!(
            recovery_operator(&base, &above),
            ComparisonOp::Gte
        ));

        let below = [tier(ComparisonOp::Lte, 5.0), tier(ComparisonOp::Lt, 20.0)];
        assert!(matches!(recovery_operator(&base, &below), ComparisonOp::Lt));
        assert!(matches!(recovery_operator(&base, &[]), ComparisonOp::Gt));
    }
}
