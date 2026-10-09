// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 实时规则的事件入口：写入路径上的匹配器判定后，复用与周期评估相同的事件流程。

use serde_json::json;

use super::{RuleEvaluator, decision::Decision, evaluation::compute_fingerprint};
use crate::{
    domain::{
        alerting::{incident::Severity, rule::AlertRule},
        query::QueryResult,
    },
    shared::{Result, time::TimestampMicros},
};

impl RuleEvaluator {
    /// 实时规则的窗口计数满足触发条件：开事件，或把已开事件升到更高一档。
    pub async fn fire_realtime(
        &self,
        rule: &AlertRule,
        count: u64,
        severity: Option<Severity>,
        now: TimestampMicros,
    ) -> Result<()> {
        // 事件上下文从查询结果派生；实时规则的“结果”就是窗口计数。
        let result = QueryResult {
            columns: vec!["count".to_owned()],
            rows: vec![vec![json!(count)]],
            scanned_rows: count,
            took_ms: 0,
            federation: None,
        };
        let decision = Decision {
            firing: true,
            severity,
            matched: true,
            deviation_ratio: None,
            consecutive: 1,
        };
        let fingerprint = compute_fingerprint(rule);
        let existing = self
            .incidents
            .find_by_fingerprint(&rule.org_id, &fingerprint)
            .await?;
        self.on_firing(rule, &decision, existing, &result, &fingerprint, now)
            .await?;
        self.record_evaluation(rule, &decision, now).await;
        Ok(())
    }

    /// 实时规则的窗口计数不再满足触发条件：解决 open 事件。
    pub async fn clear_realtime(&self, rule: &AlertRule, now: TimestampMicros) -> Result<()> {
        let decision = Decision {
            firing: false,
            severity: None,
            matched: false,
            deviation_ratio: None,
            consecutive: 0,
        };
        let existing = self
            .incidents
            .find_by_fingerprint(&rule.org_id, &compute_fingerprint(rule))
            .await?;
        self.on_cleared(rule, existing, now).await?;
        self.record_evaluation(rule, &decision, now).await;
        Ok(())
    }
}
