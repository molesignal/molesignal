// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Prometheus 告警规则（Alertmanager 用的那种规则文件，或 Prometheus Operator 的 `PrometheusRule`）
//! 换成 MoleSignal 的规则：
//!
//! - 每条 `alert:` 一条 PromQL 规则，名称是告警名；记录规则（`record:`）不导入，列在说明里。
//! - Prometheus 的意思是「表达式返回了序列就告警」，所以语句是 `count(...)`、条件是 `>= 1`；注解或
//!   标签里用到 `$labels.x` 时按这些标签拆分（`count by (x) (...)`），每组各开一个事件。
//! - `for` 换成连续几轮，评估间隔取规则组的 `interval`（没有就 60 秒）；`keep_firing_for` 不支持。
//! - 标签 `severity` 定严重度（critical、page → 严重；error、high → 错误；warning、warn、medium → 警告；
//!   info、low、none → 提示），其余标签与注解原样带上；说明取 `description` 或 `summary` 注解。

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use super::bundle::PortableRule;
use crate::{
    domain::{
        alerting::{
            incident::Severity,
            rule::{AlertQuery, AlertRuleKind, AlertTrigger, ComparisonOp},
        },
        query::{QueryLanguage, StreamHint},
        stream::StreamType,
    },
    shared::{Error, Result},
};

/// 没写 `interval` 的规则组多久评估一次。
const DEFAULT_INTERVAL_SECS: u32 = 60;

#[derive(Debug, Deserialize)]
struct RuleFile {
    #[serde(default)]
    groups: Vec<RuleGroup>,
    /// `PrometheusRule` 自定义资源：规则组在 `spec` 下。
    #[serde(default)]
    spec: Option<RuleSpec>,
}

#[derive(Debug, Deserialize)]
struct RuleSpec {
    #[serde(default)]
    groups: Vec<RuleGroup>,
}

#[derive(Debug, Deserialize)]
struct RuleGroup {
    name: String,
    #[serde(default)]
    interval: Option<String>,
    #[serde(default)]
    rules: Vec<PromRule>,
}

#[derive(Debug, Deserialize)]
struct PromRule {
    #[serde(default)]
    alert: Option<String>,
    #[serde(default)]
    record: Option<String>,
    expr: serde_yaml::Value,
    #[serde(default, rename = "for")]
    pending: Option<String>,
    #[serde(default)]
    keep_firing_for: Option<String>,
    #[serde(default)]
    labels: BTreeMap<String, String>,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
}

/// 是不是 Prometheus 规则文件：顶层有 `groups`，或是 `PrometheusRule`。
pub fn looks_like(text: &str) -> bool {
    serde_yaml::from_str::<RuleFile>(text)
        .map(|file| {
            !file.groups.is_empty() || file.spec.is_some_and(|spec| !spec.groups.is_empty())
        })
        .unwrap_or(false)
}

/// 读出规则，另附导不进来的说明（记录规则、`keep_firing_for`）。
pub fn parse(text: &str) -> Result<(Vec<PortableRule>, Vec<String>)> {
    let file: RuleFile = serde_yaml::from_str(text)
        .map_err(|error| Error::invalid(format!("not a Prometheus rule file: {error}")))?;

    let groups = match file.spec {
        Some(spec) if file.groups.is_empty() => spec.groups,
        _ => file.groups,
    };

    let mut rules = Vec::new();
    let mut notes = Vec::new();
    let mut names: BTreeMap<String, usize> = BTreeMap::new();

    for group in groups {
        let interval = match &group.interval {
            Some(text) => duration_secs(text)?.max(1),
            None => DEFAULT_INTERVAL_SECS,
        };

        for rule in group.rules {
            if let Some(record) = &rule.record {
                notes.push(format!(
                    "recording rule `{record}` in group `{}` is not imported",
                    group.name
                ));
                continue;
            }

            let Some(alert) = rule.alert.clone() else {
                continue;
            };

            if rule.keep_firing_for.is_some() {
                notes.push(format!(
                    "`keep_firing_for` of `{alert}` is not supported and was left out"
                ));
            }

            let mut portable = alert_rule(&alert, &rule, interval)?;

            // 同名的告警出现在几个组里：后面的带上组名。
            let seen = names.entry(alert.clone()).or_default();
            *seen += 1;

            if *seen > 1 {
                portable.name = format!("{alert} ({})", group.name);
            }

            rules.push(portable);
        }
    }

    Ok((rules, notes))
}

fn alert_rule(alert: &str, rule: &PromRule, interval: u32) -> Result<PortableRule> {
    let expr = match &rule.expr {
        serde_yaml::Value::String(text) => text.trim().to_string(),
        other => serde_yaml::to_string(other)
            .unwrap_or_default()
            .trim()
            .to_string(),
    };

    if expr.is_empty() {
        return Err(Error::invalid(format!("alert `{alert}` has no expr")));
    }

    let metric = first_metric(&expr).ok_or_else(|| {
        Error::invalid(format!("alert `{alert}`: no metric name found in its expr"))
    })?;

    let split: Vec<String> = referenced_labels(rule).into_iter().collect();
    let statement = if split.is_empty() {
        format!("count({expr})")
    } else {
        format!("count by ({}) ({expr})", split.join(", "))
    };

    let pending = match &rule.pending {
        Some(text) => duration_secs(text)?,
        None => 0,
    };

    let mut labels = rule.labels.clone();
    let severity = labels.remove("severity").map(|text| severity_of(&text));

    let description = rule
        .annotations
        .get("description")
        .or_else(|| rule.annotations.get("summary"))
        .cloned()
        .unwrap_or_default();

    Ok(PortableRule {
        name: alert.to_string(),
        description,
        enabled: true,
        kind: AlertRuleKind::Scheduled,
        query: AlertQuery {
            language: QueryLanguage::Promql,
            statement,
            period_secs: interval,
            stream: Some(StreamHint {
                name: metric,
                stream_type: StreamType::METRICS,
            }),
            apm: None,
            rum: None,
            group_by: split,
            composite: None,
            forecast: None,
            outlier: None,
            heartbeat: None,
            delay_secs: None,
            discovery: None,
            home_cluster: None,
        },
        trigger: AlertTrigger {
            operator: ComparisonOp::Gte,
            threshold: 1.0,
            for_periods: pending.div_ceil(interval).max(1),
            silence_secs: 0,
            no_data: None,
            recovery: None,
            priority: None,
            urgency: None,
        },
        thresholds: Vec::new(),
        severity,
        anomaly_params: None,
        escalation_policy: None,
        labels,
        annotations: rule.annotations.clone(),
    })
}

fn severity_of(text: &str) -> Severity {
    match text.trim().to_ascii_lowercase().as_str() {
        "critical" | "crit" | "page" | "p1" | "disaster" => Severity::Critical,
        "error" | "high" | "major" | "p2" => Severity::Error,
        "info" | "low" | "none" | "informational" | "p4" | "p5" => Severity::Info,
        _ => Severity::Warning,
    }
}

/// 注解和标签的模板里用到的 `$labels.x`。
fn referenced_labels(rule: &PromRule) -> BTreeSet<String> {
    let mut found = BTreeSet::new();

    for text in rule.annotations.values().chain(rule.labels.values()) {
        let mut rest = text.as_str();

        while let Some(at) = rest.find("$labels.") {
            rest = &rest[at + "$labels.".len()..];
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();

            if !name.is_empty() && name != "__name__" {
                found.insert(name);
            }
        }
    }

    found
}

/// PromQL 里第一个指标名：不是函数、聚合、关键字，也不在标签匹配、区间或引号里。
fn first_metric(expr: &str) -> Option<String> {
    const WORDS: [&str; 26] = [
        "by",
        "without",
        "on",
        "ignoring",
        "group_left",
        "group_right",
        "offset",
        "bool",
        "and",
        "or",
        "unless",
        "atan2",
        "inf",
        "nan",
        "sum",
        "avg",
        "min",
        "max",
        "count",
        "stddev",
        "stdvar",
        "topk",
        "bottomk",
        "quantile",
        "count_values",
        "group",
    ];

    let chars: Vec<char> = expr.chars().collect();
    let mut index = 0;
    // `{...}` 里是标签匹配，`[...]` 里是区间。
    let mut depth = 0;
    let mut quote: Option<char> = None;
    let mut grouping = false;

    while index < chars.len() {
        let c = chars[index];

        if let Some(open) = quote {
            if c == '\\' {
                index += 2;
                continue;
            }

            if c == open {
                quote = None;
            }

            index += 1;
            continue;
        }

        match c {
            '"' | '\'' | '`' => quote = Some(c),
            '{' | '[' => depth += 1,
            '}' | ']' => depth -= 1,
            '(' if grouping => {
                // `by (a, b)` 里的是标签名：跳到右括号。
                while index < chars.len() && chars[index] != ')' {
                    index += 1;
                }

                grouping = false;
            }
            c if depth == 0 && (c.is_ascii_alphabetic() || c == '_' || c == ':') => {
                let start = index;
                // 紧跟数字的是时长的单位或数字的指数（`5m`、`1e3`）。
                let after_digit = start > 0 && chars[start - 1].is_ascii_digit();

                while index < chars.len()
                    && (chars[index].is_ascii_alphanumeric()
                        || chars[index] == '_'
                        || chars[index] == ':')
                {
                    index += 1;
                }

                let word: String = chars[start..index].iter().collect();
                let lower = word.to_ascii_lowercase();
                let next = chars[index..].iter().find(|c| !c.is_whitespace());

                if matches!(
                    lower.as_str(),
                    "by" | "without" | "on" | "ignoring" | "group_left" | "group_right"
                ) {
                    grouping = true;
                    continue;
                }

                if !after_digit && next != Some(&'(') && !WORDS.contains(&lower.as_str()) {
                    return Some(word);
                }

                continue;
            }
            _ => {}
        }

        index += 1;
    }

    None
}

/// Prometheus 的时长：`90s`、`5m`、`1h30m`、`1d`、`1w`，换成秒。
fn duration_secs(text: &str) -> Result<u32> {
    let text = text.trim();
    let mut total: u64 = 0;
    let mut number = String::new();
    let mut chars = text.chars().peekable();

    if text.is_empty() {
        return Err(Error::invalid("an empty duration"));
    }

    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            number.push(c);
            continue;
        }

        let unit: u64 = match c {
            'm' if chars.peek() == Some(&'s') => {
                chars.next();
                0
            }
            's' => 1,
            'm' => 60,
            'h' => 3_600,
            'd' => 86_400,
            'w' => 604_800,
            'y' => 31_536_000,
            _ => return Err(Error::invalid(format!("`{text}` is not a duration"))),
        };

        let value: u64 = number
            .parse()
            .map_err(|_| Error::invalid(format!("`{text}` is not a duration")))?;
        number.clear();
        total += value * unit;
    }

    if !number.is_empty() {
        return Err(Error::invalid(format!("`{text}` has no unit")));
    }

    u32::try_from(total).map_err(|_| Error::invalid(format!("`{text}` is too long")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: &str = r#"
groups:
  - name: api
    interval: 30s
    rules:
      - record: job:http_errors:rate5m
        expr: sum(rate(http_requests_total{code=~"5.."}[5m])) by (job)
      - alert: HighErrorRate
        expr: sum(rate(http_requests_total{code=~"5.."}[5m])) by (instance) > 0.5
        for: 2m
        labels:
          severity: page
          team: payments
        annotations:
          summary: "High error rate on {{ $labels.instance }}"
          runbook_url: https://runbooks.example.com/errors
      - alert: TargetDown
        expr: up == 0
        keep_firing_for: 5m
  - name: other
    rules:
      - alert: TargetDown
        expr: up{job="db"} == 0
"#;

    #[test]
    fn alerts_become_promql_rules_that_fire_when_their_expression_returns_series() {
        let (rules, notes) = parse(RULES).unwrap();

        assert_eq!(rules.len(), 3);
        assert_eq!(notes.len(), 2, "{notes:?}");

        let errors = &rules[0];
        assert_eq!(errors.name, "HighErrorRate");
        assert_eq!(errors.query.language, QueryLanguage::Promql);
        assert_eq!(
            errors.query.statement,
            "count by (instance) (sum(rate(http_requests_total{code=~\"5..\"}[5m])) by (instance) > 0.5)"
        );
        assert_eq!(errors.query.group_by, vec!["instance".to_string()]);
        assert_eq!(errors.query.period_secs, 30);
        assert_eq!(
            errors.query.stream.as_ref().unwrap().name,
            "http_requests_total"
        );
        assert_eq!(errors.trigger.for_periods, 4);
        assert_eq!(errors.severity, Some(Severity::Critical));
        assert_eq!(
            errors.labels.get("team").map(String::as_str),
            Some("payments")
        );
        assert!(!errors.labels.contains_key("severity"));
        assert_eq!(
            errors.description,
            "High error rate on {{ $labels.instance }}"
        );

        let down = &rules[1];
        assert_eq!(down.query.statement, "count(up == 0)");
        assert_eq!(down.query.period_secs, 30);
        assert_eq!(down.trigger.for_periods, 1);
        assert_eq!(down.severity, None);

        // 没写 `interval` 的规则组按 60 秒。
        let other = &rules[2];
        assert_eq!(other.name, "TargetDown (other)");
        assert_eq!(other.query.period_secs, 60);
    }

    #[test]
    fn a_prometheus_rule_resource_is_read_from_its_spec() {
        let resource = r#"
apiVersion: monitoring.coreos.com/v1
kind: PrometheusRule
metadata:
  name: node
spec:
  groups:
    - name: node
      rules:
        - alert: DiskFull
          expr: node_filesystem_avail_bytes / node_filesystem_size_bytes < 0.05
          labels:
            severity: warning
"#;

        assert!(looks_like(resource));
        let (rules, _) = parse(resource).unwrap();
        assert_eq!(
            rules[0].query.stream.as_ref().unwrap().name,
            "node_filesystem_avail_bytes"
        );
        assert_eq!(rules[0].severity, Some(Severity::Warning));
        assert!(!looks_like(
            "{\"format\": \"molesignal.alert-rules/v1\", \"rules\": []}"
        ));
    }

    #[test]
    fn durations_and_metric_names_are_read_the_prometheus_way() {
        assert_eq!(duration_secs("90s").unwrap(), 90);
        assert_eq!(duration_secs("1h30m").unwrap(), 5_400);
        assert_eq!(duration_secs("500ms").unwrap(), 0);
        assert!(duration_secs("5").is_err());
        assert!(duration_secs("5x").is_err());

        assert_eq!(
            first_metric("rate(errors_total[5m])").as_deref(),
            Some("errors_total")
        );
        assert_eq!(
            first_metric("sum by (job) (rate(http_requests_total{job=\"api\"}[5m]))").as_deref(),
            Some("http_requests_total")
        );
        assert_eq!(first_metric("vector(1)"), None);
        assert_eq!(
            first_metric("rate(errors_total[5m] offset 1h) > 1e3").as_deref(),
            Some("errors_total")
        );
        assert_eq!(
            first_metric("max without (pod) (kube_pod_status_ready{condition=\"false\"})")
                .as_deref(),
            Some("kube_pod_status_ready")
        );
    }
}
