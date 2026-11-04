// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 实时规则的事件入口：写入路径上的匹配器判定后，复用与周期评估相同的事件流程。

use serde_json::json;

use super::{RuleEvaluator, decision::Decision, groups::Subject};
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
            consecutive: 1,
            ..Decision::quiet()
        };

        self.settle(rule, &decision, &result, &Subject::rule(rule), now)
            .await?;
        self.record_evaluation(rule, &decision, now).await;
        Ok(())
    }

    /// 接手一条实时规则时读它的状态：有进行中的事件就返回事件的严重度。
    pub async fn realtime_open(&self, rule: &AlertRule) -> Result<Option<Severity>> {
        let subject = Subject::rule(rule);

        let existing = self
            .incidents
            .find_by_fingerprint(&rule.org_id, &subject.fingerprint)
            .await?;

        Ok(existing
            .filter(|incident| incident.is_active())
            .map(|incident| incident.severity))
    }

    /// 实时规则的窗口计数不再满足触发条件：解决 open 事件。
    pub async fn clear_realtime(&self, rule: &AlertRule, now: TimestampMicros) -> Result<()> {
        let decision = Decision::quiet();
        let subject = Subject::rule(rule);

        let existing = self
            .incidents
            .find_by_fingerprint(&rule.org_id, &subject.fingerprint)
            .await?;

        self.on_cleared(rule, existing, &subject.state_key, now)
            .await?;
        self.record_evaluation(rule, &decision, now).await;
        Ok(())
    }
}
