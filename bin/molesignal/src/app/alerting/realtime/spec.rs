// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 实时规则的编译：把 `AlertRule` 的查询与触发条件变成可在写入路径上求值的 [`RealtimeSpec`]。
//!
//! 实时规则的语句形态固定为 `SELECT count(*) FROM <stream> [WHERE <condition>]`：统计最近
//! `period_secs` 秒内写入该数据流且满足条件的记录数，并与触发阈值比较。

use sqlparser::{
    ast::{
        Expr, FunctionArg, FunctionArgExpr, FunctionArguments, ObjectName, Select, SelectItem,
        SetExpr, Statement, TableFactor, TableWithJoins,
    },
    dialect::GenericDialect,
    parser::Parser,
};

use super::predicate::Predicate;
use crate::{
    domain::{
        alerting::{
            incident::Severity,
            rule::{AlertRule, ComparisonOp},
        },
        query::QueryLanguage,
        stream::StreamType,
    },
    shared::{Error, Result},
};

/// 实时规则窗口的上限（秒）。窗口按秒分桶保存计数，上限同时约束内存。
pub const MAX_WINDOW_SECS: u32 = 3600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CountOp {
    Gt,
    Gte,
}

/// 一档触发条件：窗口内命中数满足比较时，事件按 `severity` 定级（`None` = 由规则的
/// `severity` 与标签决定）。
#[derive(Debug, Clone, PartialEq)]
struct Tier {
    severity: Option<Severity>,
    op: CountOp,
    threshold: f64,
}

impl Tier {
    fn holds(&self, count: u64) -> bool {
        let count = count as f64;

        match self.op {
            CountOp::Gt => count > self.threshold,
            CountOp::Gte => count >= self.threshold,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct RealtimeSpec {
    pub(super) stream: String,
    pub(super) stream_type: StreamType,
    pub(super) predicate: Option<Predicate>,
    pub(super) window_secs: u32,
    tiers: Vec<Tier>,
}

/// 窗口计数命中的档位：外层 `None` 表示没有命中；内层是该档的严重度。
pub(super) type Level = Option<Option<Severity>>;

impl RealtimeSpec {
    pub(super) fn from_rule(rule: &AlertRule) -> std::result::Result<Self, String> {
        if rule.query.language != QueryLanguage::Sql {
            return Err("the query language must be sql".into());
        }

        let hint = rule
            .query
            .stream
            .as_ref()
            .ok_or("query.stream is required")?;

        if rule.query.period_secs == 0 || rule.query.period_secs > MAX_WINDOW_SECS {
            return Err(format!(
                "query.period_secs must be between 1 and {MAX_WINDOW_SECS}"
            ));
        }

        let predicate = parse_statement(&rule.query.statement, &hint.name)?;

        Ok(Self {
            stream: hint.name.clone(),
            stream_type: hint.stream_type,
            predicate,
            window_secs: rule.query.period_secs,
            tiers: tiers(rule)?,
        })
    }

    /// 窗口内有 `count` 条命中时触发的最高一档。
    pub(super) fn level(&self, count: u64) -> Level {
        self.tiers
            .iter()
            .filter(|tier| tier.holds(count))
            .map(|tier| tier.severity)
            .max()
    }

    pub(super) fn matches(&self, fields: &serde_json::Map<String, serde_json::Value>) -> bool {
        self.predicate
            .as_ref()
            .is_none_or(|predicate| predicate.matches(fields))
    }
}

/// 写入路径上的匹配器负责 `rule` 时返回 `true`。其余 `real_time` 规则（语句形态不被匹配器
/// 支持）仍由周期评估器按查询结果评估。
pub fn is_matched_at_intake(rule: &AlertRule) -> bool {
    rule.kind == crate::domain::alerting::rule::AlertRuleKind::RealTime
        && RealtimeSpec::from_rule(rule).is_ok()
}

/// 创建或更新 `real_time` 规则时校验它能在写入路径上运行。
pub fn validate(rule: &AlertRule) -> Result<()> {
    if rule.kind != crate::domain::alerting::rule::AlertRuleKind::RealTime {
        return Ok(());
    }

    RealtimeSpec::from_rule(rule)
        .map(|_| ())
        .map_err(|reason| Error::invalid(format!("invalid real_time rule: {reason}")))
}

fn tiers(rule: &AlertRule) -> std::result::Result<Vec<Tier>, String> {
    if rule.anomaly_params.is_some() {
        return Err("anomaly_params applies to anomaly rules only".into());
    }

    let tier =
        |severity: Option<Severity>, operator: ComparisonOp, threshold: f64, for_periods: u32| {
            let op = match operator {
                ComparisonOp::Gt => CountOp::Gt,
                ComparisonOp::Gte => CountOp::Gte,
                _ => return Err("the trigger operator must be gt or gte".to_string()),
            };

            if !threshold.is_finite() || threshold < 0.0 {
                return Err("the trigger threshold must be a non-negative number".to_string());
            }

            if for_periods > 1 {
                return Err(
                    "for_periods must be 0 or 1; a real_time rule fires as soon as the \
                        window count meets the trigger"
                        .to_string(),
                );
            }

            Ok(Tier {
                severity,
                op,
                threshold,
            })
        };

    if rule.thresholds.is_empty() {
        let trigger = &rule.trigger;

        return Ok(vec![tier(
            None,
            trigger.operator,
            trigger.threshold,
            trigger.for_periods,
        )?]);
    }

    rule.thresholds
        .iter()
        .map(|t| tier(Some(t.severity), t.operator, t.threshold, t.for_periods))
        .collect()
}

/// 解析语句并返回 `WHERE` 条件。
fn parse_statement(
    statement: &str,
    stream: &str,
) -> std::result::Result<Option<Predicate>, String> {
    const SHAPE: &str = "the statement must be `SELECT count(*) FROM <stream> [WHERE <condition>]`";
    let mut statements =
        Parser::parse_sql(&GenericDialect, statement).map_err(|error| format!("{error}"))?;

    let (Some(Statement::Query(mut query)), true) = (statements.pop(), statements.is_empty())
    else {
        return Err(SHAPE.into());
    };

    let SetExpr::Select(select) = query.body.as_mut() else {
        return Err(SHAPE.into());
    };

    if !matches!(select.projection.as_slice(),
        [SelectItem::UnnamedExpr(expr)] | [SelectItem::ExprWithAlias { expr, .. }] if is_count_all(expr))
    {
        return Err(SHAPE.into());
    }

    let table = single_table(select).ok_or(SHAPE)?;

    if table != stream {
        return Err(format!(
            "the statement selects from `{table}` but query.stream is `{stream}`"
        ));
    }

    // 去掉 WHERE 与别名后，其余部分必须与规范形态逐字一致：GROUP BY、JOIN、ORDER BY、
    // LIMIT 等任何额外子句都会让两者不同。
    let selection = select.selection.take();
    let canonical = format!("SELECT count(*) FROM {}", select.from[0]);
    select.projection = canonical_projection()?;

    if query.to_string() != canonical {
        return Err(SHAPE.into());
    }

    selection.as_ref().map(Predicate::compile).transpose()
}

fn canonical_projection() -> std::result::Result<Vec<SelectItem>, String> {
    let mut statements =
        Parser::parse_sql(&GenericDialect, "SELECT count(*)").map_err(|error| error.to_string())?;

    match statements.pop() {
        Some(Statement::Query(query)) => match *query.body {
            SetExpr::Select(select) => Ok(select.projection),
            _ => Err("canonical projection".into()),
        },
        _ => Err("canonical projection".into()),
    }
}

fn is_count_all(expr: &Expr) -> bool {
    let Expr::Function(function) = expr else {
        return false;
    };

    let FunctionArguments::List(list) = &function.args else {
        return false;
    };

    object_name_ident(&function.name).is_some_and(|name| name.eq_ignore_ascii_case("count"))
        && function.filter.is_none()
        && function.over.is_none()
        && function.null_treatment.is_none()
        && function.within_group.is_empty()
        && list.duplicate_treatment.is_none()
        && list.clauses.is_empty()
        && matches!(
            list.args.as_slice(),
            [FunctionArg::Unnamed(FunctionArgExpr::Wildcard)]
        )
}

/// `FROM` 恰好是一张无别名、无参数、无连接的表时返回表名。
fn single_table(select: &Select) -> Option<String> {
    let [
        TableWithJoins {
            relation:
                TableFactor::Table {
                    name,
                    alias: None,
                    args: None,
                    ..
                },
            joins,
        },
    ] = select.from.as_slice()
    else {
        return None;
    };

    joins.is_empty().then(|| object_name_ident(name)).flatten()
}

fn object_name_ident(name: &ObjectName) -> Option<String> {
    match name.0.as_slice() {
        [part] => part.as_ident().map(|ident| ident.value.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{
        domain::{
            alerting::rule::{AlertQuery, AlertTrigger, SeverityThreshold},
            query::StreamHint,
        },
        shared::{ids::Id, time::TimestampMicros},
    };

    fn rule(statement: &str) -> AlertRule {
        AlertRule {
            id: Id::from_string("r1"),
            org_id: Id::from_string("o1"),
            name: "errors".into(),
            description: String::new(),
            enabled: true,
            kind: crate::domain::alerting::rule::AlertRuleKind::RealTime,
            query: AlertQuery {
                language: QueryLanguage::Sql,
                statement: statement.into(),
                period_secs: 60,
                stream: Some(StreamHint {
                    name: "app".into(),
                    stream_type: StreamType::LOGS,
                }),
            },
            trigger: AlertTrigger {
                operator: ComparisonOp::Gte,
                threshold: 5.0,
                for_periods: 1,
                silence_secs: 0,
            },
            anomaly_params: None,
            thresholds: Vec::new(),
            severity: None,
            escalation_policy_id: Id::from_string("default"),
            labels: Default::default(),
            annotations: Default::default(),
            last_eval_at: None,
            last_state: Default::default(),
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn spec(statement: &str) -> std::result::Result<RealtimeSpec, String> {
        RealtimeSpec::from_rule(&rule(statement))
    }

    fn fields(value: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        match value {
            serde_json::Value::Object(map) => map,
            _ => panic!("object expected"),
        }
    }

    #[test]
    fn accepts_a_count_with_an_optional_condition() {
        assert!(spec("SELECT count(*) FROM app").is_ok());
        assert!(spec("select COUNT(*) as n from \"app\" where level = 'error'").is_ok());
        let filtered = spec("SELECT count(*) FROM app WHERE level = 'error' AND status >= 500")
            .expect("compiles");
        assert!(filtered.matches(&fields(json!({"level": "error", "status": 503}))));
        assert!(!filtered.matches(&fields(json!({"level": "error", "status": 200}))));
        let unfiltered = spec("SELECT count(*) FROM app").expect("compiles");
        assert!(unfiltered.matches(&fields(json!({}))));
    }

    #[test]
    fn rejects_statements_the_matcher_cannot_run() {
        for statement in [
            "SELECT 1",
            "SELECT * FROM app",
            "SELECT count(level) FROM app",
            "SELECT count(DISTINCT level) FROM app",
            "SELECT count(*), count(*) FROM app",
            "SELECT count(*) FROM app GROUP BY level",
            "SELECT count(*) FROM app LIMIT 1",
            "SELECT count(*) FROM app a",
            "SELECT count(*) FROM app JOIN other ON true",
            "SELECT count(*) FROM (SELECT * FROM app)",
            "SELECT count(*) FROM app HAVING count(*) > 1",
            "SELECT count(*) FILTER (WHERE level = 'x') FROM app",
            "SELECT count(*) FROM other",
            "SELECT count(*) FROM app; SELECT count(*) FROM app",
            "not sql",
        ] {
            assert!(spec(statement).is_err(), "must be rejected: {statement}");
        }
    }

    #[test]
    fn validates_window_trigger_and_language() {
        let mut candidate = rule("SELECT count(*) FROM app");
        candidate.query.period_secs = MAX_WINDOW_SECS + 1;
        assert!(RealtimeSpec::from_rule(&candidate).is_err());
        candidate.query.period_secs = 0;
        assert!(RealtimeSpec::from_rule(&candidate).is_err());

        let mut candidate = rule("SELECT count(*) FROM app");
        candidate.trigger.operator = ComparisonOp::Lt;
        assert!(RealtimeSpec::from_rule(&candidate).is_err());

        let mut candidate = rule("SELECT count(*) FROM app");
        candidate.trigger.for_periods = 3;
        assert!(RealtimeSpec::from_rule(&candidate).is_err());

        let mut candidate = rule("SELECT count(*) FROM app");
        candidate.query.language = QueryLanguage::Promql;
        assert!(RealtimeSpec::from_rule(&candidate).is_err());

        let mut candidate = rule("SELECT count(*) FROM app");
        candidate.query.stream = None;
        assert!(RealtimeSpec::from_rule(&candidate).is_err());
    }

    #[test]
    fn single_trigger_fires_when_the_window_count_meets_it() {
        let spec = spec("SELECT count(*) FROM app").unwrap();
        assert_eq!(spec.level(4), None);
        assert_eq!(spec.level(5), Some(None));

        let mut candidate = rule("SELECT count(*) FROM app");
        candidate.trigger.operator = ComparisonOp::Gt;
        let spec = RealtimeSpec::from_rule(&candidate).unwrap();
        assert_eq!(spec.level(5), None);
        assert_eq!(spec.level(6), Some(None));
    }

    #[test]
    fn tiers_pick_the_highest_satisfied_severity() {
        let mut candidate = rule("SELECT count(*) FROM app");
        candidate.thresholds = vec![
            SeverityThreshold {
                severity: Severity::Warning,
                operator: ComparisonOp::Gte,
                threshold: 5.0,
                for_periods: 1,
            },
            SeverityThreshold {
                severity: Severity::Critical,
                operator: ComparisonOp::Gte,
                threshold: 50.0,
                for_periods: 1,
            },
        ];
        let spec = RealtimeSpec::from_rule(&candidate).unwrap();
        assert_eq!(spec.level(4), None);
        assert_eq!(spec.level(5), Some(Some(Severity::Warning)));
        assert_eq!(spec.level(80), Some(Some(Severity::Critical)));
    }

    #[test]
    fn only_compilable_real_time_rules_are_owned_by_the_matcher() {
        assert!(is_matched_at_intake(&rule("SELECT count(*) FROM app")));
        assert!(!is_matched_at_intake(&rule("SELECT avg(latency) FROM app")));
        let mut scheduled = rule("SELECT count(*) FROM app");
        scheduled.kind = crate::domain::alerting::rule::AlertRuleKind::Scheduled;
        assert!(!is_matched_at_intake(&scheduled));
        // 校验只约束 real_time 规则。
        assert!(validate(&scheduled).is_ok());
        assert!(validate(&rule("SELECT avg(latency) FROM app")).is_err());
    }
}
