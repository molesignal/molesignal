// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 在规则的评估窗口上执行规则查询。

use std::time::Duration;

use super::RuleEvaluator;
use crate::{
    domain::{
        alerting::rule::AlertRule,
        query::{QueryRequest, QueryResult, StreamHint},
    },
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

impl RuleEvaluator {
    /// 跑一条规则查询在 [from, to] 窗口的结果；超时 / 错误返回 None 并 warn。
    pub(super) async fn run_window(
        &self,
        rule: &AlertRule,
        stream: &StreamHint,
        from: TimestampMicros,
        to: TimestampMicros,
    ) -> Option<QueryResult> {
        match self.run_window_result(rule, stream, from, to).await {
            Ok(result) => Some(result),
            Err(error) => {
                tracing::warn!(rule_id = %rule.id, error = %error, "rule query failed");
                None
            }
        }
    }

    pub(super) async fn run_window_result(
        &self,
        rule: &AlertRule,
        stream: &StreamHint,
        from: TimestampMicros,
        to: TimestampMicros,
    ) -> Result<QueryResult> {
        let req = QueryRequest {
            org_id: rule.org_id.clone(),
            language: rule.query.language,
            statement: rule.query.statement.clone(),
            time_range: TimeRange::new(from, to),
            stream: Some(stream.clone()),
            limit: Some(1),
            federation_clusters: Vec::new(),
        };

        let timeout = Duration::from_secs(self.eval_timeout_secs as u64);

        tokio::time::timeout(timeout, self.query_engine.execute(req))
            .await
            .map_err(|_| Error::cancelled("alert rule test timed out"))?
    }
}
