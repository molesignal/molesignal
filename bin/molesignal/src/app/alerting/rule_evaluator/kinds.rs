// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 不和阈值比的几种规则：
//!
//! - 组合：被组合的规则里哪些有进行中的事件，代进表达式；可选把它们的事件并到组合规则的事件下。
//! - 心跳：看最后一次打卡；打卡时马上判定一轮。
//! - 预测：这一轮的值记进序列（规则刚建好或查询改过时先补一批历史点），往后推，预计越线就触发。
//! - 离群：按列拆分的各组相互比较（见 `groups`）。
//!
//! 它们的判定都写成 [`Reading::Verdict`]，去抖、恢复和事件生命周期与别的规则一样。

use std::collections::{BTreeMap, HashSet};

use chrono::{TimeZone, Utc};

use super::{
    RuleEvaluator, WORK,
    decision::Decision,
    evaluation::first_cell_f64,
    groups::{Group, Subject},
    round::{Reading, advance},
};
use crate::{
    app::cluster::record_overlap_skipped,
    domain::{
        alerting::{
            forecast::{
                FORECAST_BACKFILL_POINTS, ForecastParams, ForecastSeries, SeriesPoint,
                forecast_crossing,
            },
            heartbeat::{HeartbeatStatus, MAX_HEARTBEAT_MESSAGE_CHARS},
            incident::Incident,
            outlier::outlier_scores,
            repositories::AlertRuleEvalState,
            rule::{AlertRule, AlertRuleKind},
        },
        query::{QueryResult, StreamHint},
    },
    shared::{Result, time::TimestampMicros},
};

/// 时间写成 UTC 的 RFC 3339。
fn utc(at: i64) -> String {
    Utc.timestamp_micros(at)
        .single()
        .map(|time| time.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_default()
}

fn empty_result() -> QueryResult {
    QueryResult {
        columns: Vec::new(),
        rows: Vec::new(),
        scanned_rows: 0,
        took_ms: 0,
        federation: None,
    }
}

/// 预测规则的序列跟着什么走：查询、窗口和回看改了，序列就要重新补。
fn forecast_signature(rule: &AlertRule, params: &ForecastParams) -> String {
    let basis = serde_json::json!({
        "language": rule.query.language,
        "statement": rule.query.statement,
        "stream": rule.query.stream,
        "apm": rule.query.apm,
        "rum": rule.query.rum,
        "periodSecs": rule.query.period_secs,
        "lookbackDays": params.lookback_days,
    });

    blake3::hash(basis.to_string().as_bytes())
        .to_hex()
        .to_string()
}

/// 离群规则这一轮每组的读数：各组相互比较，离群的组写上偏离程度。有值的组不够时 `None`，这一轮
/// 不下结论。
pub(super) fn outlier_readings(rule: &AlertRule, groups: &[Group]) -> Option<Vec<Reading>> {
    let params = rule.query.outlier.as_ref()?;
    let values = groups.iter().map(|group| group.value).collect::<Vec<_>>();

    let readings = outlier_scores(&values)?
        .into_iter()
        .map(|score| match score {
            None => Reading::Missing,
            Some(score) => {
                let firing = params.is_outlier(score);
                let mut annotations = BTreeMap::new();

                if firing {
                    annotations.insert("outlier_score".to_string(), format!("{score:.1}"));
                }

                Reading::Verdict {
                    firing,
                    annotations,
                }
            }
        })
        .collect();

    Some(readings)
}

impl RuleEvaluator {
    /// 组合与心跳规则的一轮：不跑查询。
    pub(super) async fn eval_without_query(
        &self,
        rule: &AlertRule,
        now: TimestampMicros,
    ) -> Result<()> {
        if let Some(gap) = self.cycle_gap_micros
            && !self.eval_state.claim_cycle(&rule.id, now, gap).await?
        {
            record_overlap_skipped(WORK);
            return Ok(());
        }

        self.settle_verdict(rule, now).await
    }

    /// 判定组合或心跳规则这一轮，推进状态，落到事件上。
    async fn settle_verdict(&self, rule: &AlertRule, now: TimestampMicros) -> Result<()> {
        let Some((firing, annotations)) = self.verdict(rule, now).await? else {
            return Ok(());
        };

        let mut state = self
            .eval_state
            .get(&rule.id, "")
            .await?
            .unwrap_or_else(|| AlertRuleEvalState::fresh(&rule.id, "", BTreeMap::new(), now));

        let decision = advance(
            rule,
            &mut state,
            &Reading::Verdict {
                firing,
                annotations,
            },
            now,
        );
        self.eval_state.upsert_state(state).await?;

        self.settle(rule, &decision, &empty_result(), &Subject::rule(rule), now)
            .await?;
        self.record_evaluation(rule, &decision, now).await;
        Ok(())
    }

    /// 组合或心跳规则这一轮触发没有，和要写进事件的说明。缺参数、还没有打卡地址时没有判定。
    async fn verdict(
        &self,
        rule: &AlertRule,
        now: TimestampMicros,
    ) -> Result<Option<(bool, BTreeMap<String, String>)>> {
        let mut annotations = BTreeMap::new();

        match rule.kind {
            AlertRuleKind::Composite => {
                let Some(composite) = &rule.query.composite else {
                    tracing::warn!(rule_id = %rule.id, "composite rule without composite query; skipped");
                    return Ok(None);
                };

                let mut active = HashSet::new();

                for id in composite.rules.values() {
                    if !self
                        .incidents
                        .list_active_for_rule(&rule.org_id, id)
                        .await?
                        .is_empty()
                    {
                        active.insert(id.clone());
                    }
                }

                let letters = composite
                    .rules
                    .iter()
                    .filter(|(_, id)| active.contains(*id))
                    .map(|(letter, _)| letter.as_str())
                    .collect::<Vec<_>>();

                annotations.insert("composite_active".into(), letters.join(", "));

                Ok(Some((
                    composite.holds(|id| active.contains(id)),
                    annotations,
                )))
            }
            AlertRuleKind::Heartbeat => {
                let (Some(params), Some(heartbeats)) = (&rule.query.heartbeat, &self.heartbeats)
                else {
                    return Ok(None);
                };

                let Some(beat) = heartbeats.get(&rule.id).await? else {
                    return Ok(None);
                };

                annotations.insert("heartbeat_due_at".into(), utc(beat.due_at(params).0));
                annotations.insert(
                    "heartbeat_last_checkin_at".into(),
                    beat.last_checkin_at.map(|at| utc(at.0)).unwrap_or_default(),
                );
                annotations.insert("heartbeat_status".into(), beat.last_status.as_str().into());

                if !beat.last_message.is_empty() {
                    annotations.insert("heartbeat_message".into(), beat.last_message.clone());
                }

                Ok(Some((beat.alarming(params, now), annotations)))
            }
            _ => Ok(None),
        }
    }

    /// 心跳规则的打卡：记下这一次，马上判定一轮（报失败马上开事件，正常打卡马上恢复）。Token 的哈希
    /// 对不上任何规则时返回 `false`。
    pub async fn check_in(
        &self,
        token_hash: &str,
        status: HeartbeatStatus,
        message: &str,
        now: TimestampMicros,
    ) -> Result<bool> {
        let Some(heartbeats) = &self.heartbeats else {
            return Ok(false);
        };

        let Some(beat) = heartbeats.find_by_token_hash(token_hash).await? else {
            return Ok(false);
        };

        let message = message
            .trim()
            .chars()
            .take(MAX_HEARTBEAT_MESSAGE_CHARS)
            .collect::<String>();

        heartbeats
            .record_checkin(&beat.rule_id, now, status, &message)
            .await?;

        let rule = self.rules.get(&beat.rule_id).await?;

        if rule.enabled && rule.kind == AlertRuleKind::Heartbeat {
            self.settle_verdict(&rule, now).await?;
        }

        Ok(true)
    }

    /// 预测规则的一轮：这一轮的值记进序列，往后推。序列还不够时只把序列存下，返回 `None`，这一轮
    /// 不下结论。
    pub(super) async fn decide_forecast(
        &self,
        rule: &AlertRule,
        stream: &StreamHint,
        current: &QueryResult,
        now: TimestampMicros,
        period_us: i64,
    ) -> Result<Option<Decision>> {
        let Some(params) = &rule.query.forecast else {
            tracing::warn!(rule_id = %rule.id, "forecast rule without forecast params; skipped");
            return Ok(None);
        };

        let mut state = self
            .eval_state
            .get(&rule.id, "")
            .await?
            .unwrap_or_else(|| AlertRuleEvalState::fresh(&rule.id, "", BTreeMap::new(), now));

        let signature = forecast_signature(rule, params);

        if state.series.needs_fill(&signature) {
            state.series = ForecastSeries {
                points: self.backfill(rule, stream, params, now, period_us).await,
                signature,
                filled_at: Some(now.0),
            };
        }

        if let Some(value) = first_cell_f64(current).filter(|value| value.is_finite()) {
            state.series.record(
                SeriesPoint { at: now.0, value },
                params.step_micros(rule.query.period_secs),
                params.lookback_micros(),
            );
        }

        let Some(crossing) = forecast_crossing(
            params,
            &state.series.points,
            now.0,
            rule.trigger.operator,
            rule.trigger.threshold,
        ) else {
            self.eval_state.upsert_state(state).await?;
            return Ok(None);
        };

        let mut annotations = BTreeMap::new();

        if let Some(crossing) = crossing {
            annotations.insert("forecast_crossing_at".into(), utc(crossing.at));
            annotations.insert("forecast_value".into(), format!("{:.2}", crossing.value));
            annotations.insert("forecast_model".into(), params.model.as_str().into());
        }

        let decision = advance(
            rule,
            &mut state,
            &Reading::Verdict {
                firing: crossing.is_some(),
                annotations,
            },
            now,
        );
        self.eval_state.upsert_state(state).await?;

        Ok(Some(decision))
    }

    /// 在回看时间里均匀补一批历史点，每个点跑一次规则查询。一次查询失败就不再补，免得一直卡在超时
    /// 上；序列以后靠每一轮的值补齐。
    async fn backfill(
        &self,
        rule: &AlertRule,
        stream: &StreamHint,
        params: &ForecastParams,
        now: TimestampMicros,
        period_us: i64,
    ) -> Vec<SeriesPoint> {
        let spacing = params.lookback_micros() / FORECAST_BACKFILL_POINTS as i64;
        let mut points = Vec::with_capacity(FORECAST_BACKFILL_POINTS);

        for index in (1..=FORECAST_BACKFILL_POINTS as i64).rev() {
            let to = now.0 - index * spacing;

            let Some(result) = self
                .run_window(
                    rule,
                    stream,
                    TimestampMicros(to - period_us),
                    TimestampMicros(to),
                )
                .await
            else {
                tracing::warn!(rule_id = %rule.id, "forecast history stopped at a failed query");
                break;
            };

            if let Some(value) = first_cell_f64(&result).filter(|value| value.is_finite()) {
                points.push(SeriesPoint { at: to, value });
            }
        }

        points
    }

    /// 组合规则的事件开着时，把被组合的规则还没并进来的进行中事件并到它下面。失败只记日志。
    pub(super) async fn merge_children(&self, rule: &AlertRule, incident: &Incident) {
        let (Some(composite), Some(merger)) = (&rule.query.composite, &self.merger) else {
            return;
        };

        if rule.kind != AlertRuleKind::Composite || !composite.merge_children {
            return;
        }

        let mut children = Vec::new();

        for id in composite.rules.values() {
            match self.incidents.list_active_for_rule(&rule.org_id, id).await {
                Ok(active) => children.extend(
                    active
                        .into_iter()
                        .filter(|child| child.handling.merged_into.is_none())
                        .map(|child| child.id),
                ),
                Err(error) => {
                    tracing::warn!(rule_id = %rule.id, %error, "incidents of a combined rule could not be read");
                }
            }
        }

        if children.is_empty() {
            return;
        }

        if let Err(error) = merger
            .merge_children(&rule.org_id, &incident.id, &children)
            .await
        {
            tracing::warn!(rule_id = %rule.id, incident_id = %incident.id, %error, "combined incidents could not be merged");
        }
    }
}
