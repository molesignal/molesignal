// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 在规则的评估窗口上执行规则查询。

use std::time::Duration;

use super::RuleEvaluator;
use crate::{
    app::alerting::{apm_source::reading_result, rum_source},
    domain::{
        alerting::rule::{AlertRule, MAX_ALERT_GROUPS},
        query::{QueryLanguage, QueryRequest, QueryResult, StreamHint},
    },
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

/// 规则查询最多取多少行。按列拆分的 SQL 规则多取一行，看得出组数有没有超过上限。PromQL 的 limit 大于 1
/// 会改走 range 查询，而即时查询本来就返回全部 series，所以仍是 1。
pub(super) fn row_limit(rule: &AlertRule) -> usize {
    if rule.query.group_by.is_empty() || rule.query.language == QueryLanguage::Promql {
        1
    } else {
        MAX_ALERT_GROUPS + 1
    }
}

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

    /// 跑一条规则查询在 [from, to] 窗口的结果；窗口按规则的评估延迟整体往回挪（[`Self::delay_micros`]），
    /// 当前窗口、异常基线、预测补点和规则测试都一样。
    pub(super) async fn run_window_result(
        &self,
        rule: &AlertRule,
        stream: &StreamHint,
        from: TimestampMicros,
        to: TimestampMicros,
    ) -> Result<QueryResult> {
        let timeout = Duration::from_secs(self.eval_timeout_secs as u64);
        let delay = self.delay_micros(rule);
        let (from, to) = (
            TimestampMicros(from.0.saturating_sub(delay)),
            TimestampMicros(to.0.saturating_sub(delay)),
        );

        // An APM rule's figure comes from APM's aggregates, not from a query.
        if let Some(apm) = &rule.query.apm {
            let started = std::time::Instant::now();
            let source = self.apm.source()?;
            let reading = tokio::time::timeout(
                timeout,
                source.read(&rule.org_id, apm, TimeRange::new(from, to)),
            )
            .await
            .map_err(|_| Error::cancelled("APM alert rule timed out"))??;

            return Ok(reading_result(
                reading,
                started.elapsed().as_millis() as u64,
            ));
        }

        // 自动发现规则的「结果」是命中的进行中的发现，不跑查询。
        if let Some(query) = &rule.query.discovery {
            let started = std::time::Instant::now();
            let findings = self
                .discovery
                .as_ref()
                .ok_or_else(|| Error::unavailable("discovery is not running on this node"))?
                .list_open(&rule.org_id)
                .await?;

            return Ok(crate::app::alerting::discovery::findings_result(
                &findings,
                query,
                started.elapsed().as_millis() as u64,
            ));
        }

        // A RUM rule's figure comes from the RUM data the pages show, read the way they read it.
        if let Some(rum) = &rule.query.rum {
            let started = std::time::Instant::now();
            let source = self
                .rum
                .as_ref()
                .ok_or_else(|| Error::unavailable("RUM is not running on this node"))?;
            let reading = tokio::time::timeout(
                timeout,
                source.read(&rule.org_id, rum, TimeRange::new(from, to)),
            )
            .await
            .map_err(|_| Error::cancelled("RUM alert rule timed out"))??;

            return Ok(rum_source::reading_result(
                &rum.application_id,
                reading,
                started.elapsed().as_millis() as u64,
            ));
        }

        let req = QueryRequest {
            org_id: rule.org_id.clone(),
            language: rule.query.language,
            statement: rule.query.statement.clone(),
            time_range: TimeRange::new(from, to),
            stream: Some(stream.clone()),
            limit: Some(row_limit(rule)),
            step: None,
            federation_clusters: Vec::new(),
        };

        tokio::time::timeout(timeout, self.query_engine.execute(req))
            .await
            .map_err(|_| Error::cancelled("alert rule test timed out"))?
    }
}
