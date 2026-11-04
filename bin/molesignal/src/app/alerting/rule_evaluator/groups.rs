// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 按列拆分的规则：查询结果里每组维度取值（主机、服务……）各自判定、各自去抖、各开各的事件。
//!
//! - 一组是维度列取值相同的行，拿第一行的值与阈值比较。值在维度列与时间列以外的第一个数值列。
//! - 一轮最多 [`MAX_ALERT_GROUPS`] 组，多出来的这一轮不评估。
//! - 每组的去抖状态记着这一组的取值。之前出现过、这一轮结果里没有的组，按规则的 `no_data`
//!   当作这一组没有值。结果被截断时这些组这一轮不动：不知道没取到的组是不是还在。
//! - 结果里没有、也没有事件或待触发状态要留着的组会被忘掉；不再跟着的组留下的事件按恢复处理。

use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::Value;

use super::{
    RuleEvaluator,
    decision::Decision,
    evaluation::{group_fingerprint, group_state_key},
    kinds::outlier_readings,
    round::{Reading, advance, with_no_data_value},
    window::row_limit,
};
use crate::{
    domain::{
        alerting::{
            repositories::AlertRuleEvalState,
            rule::{AlertRule, AlertRuleKind, MAX_ALERT_GROUPS, NoDataAction},
        },
        query::QueryResult,
    },
    shared::{Error, Result, time::TimestampMicros},
};

/// 时间列不当作值。
const TIME_COLUMNS: [&str; 4] = ["_timestamp", "ts", "time", "timestamp"];

/// 结果里缺席满一天、又没有事件开着的组被忘掉，免得留着它待触发的状态没完没了。
const FORGET_ABSENT_AFTER_MICROS: i64 = 86_400_000_000;

/// 事件跟踪的对象：整条规则（维度为空），或按列拆分的规则里的一组。
pub(super) struct Subject {
    /// 维度列（按规则里的写法）→ 这一组的取值。
    pub(super) labels: BTreeMap<String, String>,
    pub(super) fingerprint: String,
    /// 这一组连续一段没有值时开的无数据事件的指纹，与按阈值开的事件分开。
    pub(super) no_data_fingerprint: String,
    /// 去抖状态那一行的 `group_key`。
    pub(super) state_key: String,
}

impl Subject {
    pub(super) fn rule(rule: &AlertRule) -> Self {
        Self::group(rule, BTreeMap::new())
    }

    pub(super) fn group(rule: &AlertRule, labels: BTreeMap<String, String>) -> Self {
        let fingerprint = group_fingerprint(rule, &labels);

        Self {
            no_data_fingerprint: blake3::hash(format!("{fingerprint}|no_data").as_bytes())
                .to_hex()
                .to_string(),
            fingerprint,
            state_key: group_state_key(&labels),
            labels,
        }
    }
}

/// 结果里的一组。
pub(super) struct Group {
    pub(super) labels: BTreeMap<String, String>,
    /// 这一组第一行的值；没有数值时为 `None`。
    pub(super) value: Option<f64>,
    /// 这一组的行，开事件时从中取上下文。
    pub(super) result: QueryResult,
}

/// 按 `group_by` 的列把结果分组，组与组内的行都保持原来的顺序。第二个返回值为真表示组数超过上限，
/// 多出的组被丢掉了。结果有行却缺一个维度列时报错：规则的查询没有返回它要拆分的列。
pub(super) fn split_groups(
    group_by: &[String],
    result: &QueryResult,
) -> Result<(Vec<Group>, bool)> {
    if result.rows.is_empty() {
        return Ok((Vec::new(), false));
    }

    let dimensions = group_by
        .iter()
        .map(|name| {
            result
                .columns
                .iter()
                .position(|column| column.eq_ignore_ascii_case(name))
                .map(|index| (name.as_str(), index))
                .ok_or_else(|| {
                    Error::invalid(format!(
                        "the query result has no column {name} to split the rule by"
                    ))
                })
        })
        .collect::<Result<Vec<_>>>()?;

    let value_index = value_column(result, &dimensions);

    let mut groups: Vec<Group> = Vec::new();
    let mut positions: HashMap<BTreeMap<String, String>, usize> = HashMap::new();
    let mut truncated = false;

    for row in &result.rows {
        let labels = dimensions
            .iter()
            .map(|(name, index)| ((*name).to_string(), cell_text(row.get(*index))))
            .collect::<BTreeMap<_, _>>();

        if let Some(position) = positions.get(&labels) {
            groups[*position].result.rows.push(row.clone());
            continue;
        }

        if groups.len() == MAX_ALERT_GROUPS {
            truncated = true;
            continue;
        }

        let value = value_index
            .and_then(|index| row.get(index))
            .and_then(cell_f64)
            .filter(|value| !value.is_nan());

        positions.insert(labels.clone(), groups.len());

        groups.push(Group {
            labels,
            value,
            result: QueryResult {
                columns: result.columns.clone(),
                rows: vec![row.clone()],
                scanned_rows: result.scanned_rows,
                took_ms: result.took_ms,
                federation: None,
            },
        });
    }

    Ok((groups, truncated))
}

/// 值所在的列：维度列与时间列以外，第一个在某一行里是数值的列。
fn value_column(result: &QueryResult, dimensions: &[(&str, usize)]) -> Option<usize> {
    result
        .columns
        .iter()
        .enumerate()
        .find_map(|(index, column)| {
            let candidate = !dimensions.iter().any(|(_, dimension)| *dimension == index)
                && !TIME_COLUMNS
                    .iter()
                    .any(|time| column.eq_ignore_ascii_case(time))
                && result
                    .rows
                    .iter()
                    .any(|row| row.get(index).and_then(cell_f64).is_some());

            candidate.then_some(index)
        })
}

/// 维度取值的文字：字符串照原样，数字与布尔按 JSON 的写法，空值为空串。
fn cell_text(cell: Option<&Value>) -> String {
    match cell {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

fn cell_f64(cell: &Value) -> Option<f64> {
    cell.as_f64()
        .or_else(|| cell.as_i64().map(|value| value as f64))
        .or_else(|| cell.as_u64().map(|value| value as f64))
}

/// 记着的这一组是不是按规则现在的维度列分出来的；改过维度列之后，旧的组不再是这条规则的组。
fn same_columns(rule: &AlertRule, labels: &BTreeMap<String, String>) -> bool {
    labels.len() == rule.query.group_by.len()
        && rule
            .query
            .group_by
            .iter()
            .all(|name| labels.contains_key(name))
}

/// 结果里没有的一组，评估完以后还要不要记着：被人解决了这一段的无数据事件就忘掉；开着事件的
/// 留着；`alert` 要数没有值的轮数，留着；其余没有待触发的状态，或缺席满一天，忘掉。
fn forgettable(
    action: NoDataAction,
    decision: &Decision,
    dismissed: bool,
    has_incident: bool,
    now: TimestampMicros,
) -> bool {
    if dismissed {
        return true;
    }

    if has_incident || action == NoDataAction::Alert {
        return false;
    }

    let long_gone = decision
        .no_data_since
        .is_some_and(|since| now.0.saturating_sub(since.0) >= FORGET_ABSENT_AFTER_MICROS);

    decision.consecutive == 0 || long_gone
}

impl RuleEvaluator {
    /// 按列拆分的规则的一轮：每组判定并落到自己的事件上，返回整条规则的结论（有组在触发就是触发）。
    /// 一组出错只记日志，不影响其他组。离群规则的组不够相互比较时返回 `None`，这一轮不下结论。
    pub(super) async fn eval_grouped(
        &self,
        rule: &AlertRule,
        query_result: &QueryResult,
        now: TimestampMicros,
    ) -> Result<Option<Decision>> {
        let (groups, cut) = split_groups(&rule.query.group_by, query_result)?;

        let verdicts = match rule.kind {
            AlertRuleKind::Outlier => match outlier_readings(rule, &groups) {
                Some(verdicts) => Some(verdicts),
                None => return Ok(None),
            },
            _ => None,
        };

        // SQL 取到了行数上限：后面可能还有组没取到。
        let limit = row_limit(rule);
        let truncated = cut || (limit > 1 && query_result.rows.len() >= limit);

        if truncated {
            tracing::warn!(
                rule_id = %rule.id,
                max_groups = MAX_ALERT_GROUPS,
                "alert rule result has more groups than are evaluated; the rest are skipped"
            );
        }

        let mut known = self
            .eval_state
            .list_groups(&rule.id)
            .await?
            .into_iter()
            .map(|state| (state.group_key.clone(), state))
            .collect::<HashMap<_, _>>();

        let mut overall = Decision::quiet();
        let mut tracked = HashSet::new();

        for (index, group) in groups.iter().enumerate() {
            let subject = Subject::group(rule, group.labels.clone());
            tracked.insert(subject.fingerprint.clone());
            tracked.insert(subject.no_data_fingerprint.clone());

            let state = known.remove(&subject.state_key).unwrap_or_else(|| {
                AlertRuleEvalState::fresh(&rule.id, &subject.state_key, group.labels.clone(), now)
            });

            let reading = match verdicts.as_ref().and_then(|verdicts| verdicts.get(index)) {
                Some(verdict) => verdict.clone(),
                None => {
                    with_no_data_value(rule, group.value).map_or(Reading::Missing, Reading::Value)
                }
            };

            match self
                .eval_subject(rule, &subject, state, &reading, &group.result, now)
                .await
            {
                Ok((decision, _)) => overall.absorb(&decision),
                Err(error) => tracing::warn!(
                    rule_id = %rule.id,
                    group = ?group.labels,
                    %error,
                    "alert rule group eval failed"
                ),
            }
        }

        // 结果被截断时，没取到的组这一轮不动，它们的事件也不动。
        if truncated {
            return Ok(Some(overall));
        }

        let action = rule.trigger.no_data_action(rule.kind);
        let nothing = QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            scanned_rows: 0,
            took_ms: 0,
            federation: None,
        };

        let mut forget = Vec::new();
        let mut absent = Vec::new();

        for (key, state) in known {
            if !same_columns(rule, &state.group_labels) {
                forget.push(key);
                continue;
            }

            let subject = Subject::group(rule, state.group_labels.clone());
            let reading = with_no_data_value(rule, None).map_or(Reading::Missing, Reading::Value);

            match self
                .eval_subject(rule, &subject, state, &reading, &nothing, now)
                .await
            {
                Ok((decision, dismissed)) => {
                    overall.absorb(&decision);
                    absent.push((subject, decision, dismissed));
                }
                Err(error) => {
                    tracing::warn!(
                        rule_id = %rule.id,
                        group = ?subject.labels,
                        %error,
                        "alert rule group without a value could not be evaluated"
                    );

                    tracked.insert(subject.fingerprint);
                    tracked.insert(subject.no_data_fingerprint);
                }
            }
        }

        let active = self
            .incidents
            .list_active_for_rule(&rule.org_id, &rule.id)
            .await?;

        let open = active
            .iter()
            .map(|incident| incident.fingerprint.as_str())
            .collect::<HashSet<_>>();

        for (subject, decision, dismissed) in absent {
            let has_incident = open.contains(subject.fingerprint.as_str())
                || open.contains(subject.no_data_fingerprint.as_str());

            if forgettable(action, &decision, dismissed, has_incident, now) {
                forget.push(subject.state_key);
            } else {
                tracked.insert(subject.fingerprint);
                tracked.insert(subject.no_data_fingerprint);
            }
        }

        // 不再跟着的组留下的事件（组被忘掉，或规则改过维度列、标签）按恢复处理。
        for incident in active
            .into_iter()
            .filter(|incident| !tracked.contains(&incident.fingerprint))
        {
            let incident_id = incident.id.clone();

            if let Err(error) = self.resolve_open(incident, now).await {
                tracing::warn!(
                    rule_id = %rule.id,
                    %incident_id,
                    %error,
                    "incident of an alert group no longer followed could not be resolved"
                );
            }
        }

        self.eval_state.forget_groups(&rule.id, &forget).await?;
        Ok(Some(overall))
    }

    /// 一组的一轮：推进它的状态并写回，再落到它的事件上。返回这一轮的结论，以及这一段没有值期间
    /// 开的无数据事件是否已经被人解决。
    async fn eval_subject(
        &self,
        rule: &AlertRule,
        subject: &Subject,
        mut state: AlertRuleEvalState,
        reading: &Reading,
        result: &QueryResult,
        now: TimestampMicros,
    ) -> Result<(Decision, bool)> {
        let decision = advance(rule, &mut state, reading, now);
        self.eval_state.upsert_state(state).await?;

        let dismissed = self.settle(rule, &decision, result, subject, now).await?;
        Ok((decision, dismissed))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn result(columns: &[&str], rows: Vec<Vec<Value>>) -> QueryResult {
        QueryResult {
            columns: columns.iter().map(|column| (*column).to_string()).collect(),
            rows,
            scanned_rows: 0,
            took_ms: 0,
            federation: None,
        }
    }

    fn group_by(columns: &[&str]) -> Vec<String> {
        columns.iter().map(|column| (*column).to_string()).collect()
    }

    #[test]
    fn rows_are_split_by_the_named_columns_and_valued_by_the_first_number_after_them() {
        let result = result(
            &["host", "_timestamp", "errors", "p99"],
            vec![
                vec![
                    json!("web-1"),
                    json!(1_700_000_000_i64),
                    json!(12),
                    json!(0.5),
                ],
                vec![
                    json!("web-2"),
                    json!(1_700_000_000_i64),
                    json!(3),
                    json!(0.2),
                ],
                vec![
                    json!("web-1"),
                    json!(1_700_000_060_i64),
                    json!(99),
                    json!(0.9),
                ],
            ],
        );

        let (groups, truncated) = split_groups(&group_by(&["HOST"]), &result).unwrap();

        assert!(!truncated);
        assert_eq!(groups.len(), 2);
        assert_eq!(
            groups[0].labels,
            [("HOST".to_string(), "web-1".to_string())].into()
        );
        // The first row of a group carries its value; the later rows only add context.
        assert_eq!(groups[0].value, Some(12.0));
        assert_eq!(groups[0].result.rows.len(), 2);
        assert_eq!(groups[1].labels["HOST"], "web-2");
        assert_eq!(groups[1].value, Some(3.0));
    }

    #[test]
    fn promql_series_are_split_by_their_labels() {
        let result = result(
            &["value", "instance", "job"],
            vec![
                vec![json!(0.97), json!("10.0.0.1:9100"), json!("node")],
                vec![json!(0.42), json!("10.0.0.2:9100"), json!(null)],
            ],
        );

        let (groups, _) = split_groups(&group_by(&["instance", "job"]), &result).unwrap();

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].value, Some(0.97));
        assert_eq!(groups[1].labels["job"], "");
    }

    #[test]
    fn a_missing_dimension_is_an_error_only_when_there_are_rows() {
        let empty = result(&["errors"], Vec::new());
        assert!(
            split_groups(&group_by(&["host"]), &empty)
                .unwrap()
                .0
                .is_empty()
        );

        let rows = result(&["errors"], vec![vec![json!(1)]]);
        assert!(split_groups(&group_by(&["host"]), &rows).is_err());
    }

    #[test]
    fn groups_past_the_limit_are_dropped_and_reported() {
        let rows = (0..=MAX_ALERT_GROUPS)
            .map(|index| vec![json!(format!("h-{index}")), json!(index)])
            .collect();

        let (groups, truncated) =
            split_groups(&group_by(&["host"]), &result(&["host", "n"], rows)).unwrap();

        assert!(truncated);
        assert_eq!(groups.len(), MAX_ALERT_GROUPS);
    }

    #[test]
    fn a_number_dimension_is_written_as_json_and_a_text_value_is_no_value() {
        let result = result(
            &["status", "count"],
            vec![
                vec![json!(500), json!("many")],
                vec![json!(true), json!(null)],
            ],
        );

        let (groups, _) = split_groups(&group_by(&["status"]), &result).unwrap();

        assert_eq!(groups[0].labels["status"], "500");
        assert_eq!(groups[1].labels["status"], "true");
        assert!(groups.iter().all(|group| group.value.is_none()));
    }
}
