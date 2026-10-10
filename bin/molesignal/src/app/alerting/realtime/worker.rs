// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 实时规则的后台任务：执行开事件与解决命令，周期性重新加载规则并检查已触发的规则。

use std::{sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use super::{Command, RealtimeAlerts};
use crate::{
    app::alerting::RuleEvaluator, domain::alerting::repositories::AlertRuleRepository,
    shared::time::TimestampMicros,
};

/// 重新加载规则的间隔：让其它节点上的规则变更在这个时间内生效。
const RELOAD_EVERY: Duration = Duration::from_secs(10);

/// 检查已触发规则是否恢复的间隔。
const SWEEP_EVERY: Duration = Duration::from_secs(2);

pub struct RealtimeAlertWorker {
    alerts: Arc<RealtimeAlerts>,
    rules: Arc<dyn AlertRuleRepository>,
    evaluator: Arc<RuleEvaluator>,
}

impl RealtimeAlertWorker {
    pub fn new(
        alerts: Arc<RealtimeAlerts>,
        rules: Arc<dyn AlertRuleRepository>,
        evaluator: Arc<RuleEvaluator>,
    ) -> Self {
        Self {
            alerts,
            rules,
            evaluator,
        }
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(self.run())
    }

    #[tracing::instrument(
        name = "worker.realtime_alerts",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "realtime_alerts")
    )]
    async fn run(self) {
        let Some(mut inbox) = self.alerts.take_inbox() else {
            tracing::warn!("real-time alert worker is already running");
            return;
        };

        let mut reload = tokio::time::interval(RELOAD_EVERY);
        let mut sweep = tokio::time::interval(SWEEP_EVERY);

        loop {
            tokio::select! {
                command = inbox.recv() => match command {
                    Some(command) => self.execute(command).await,
                    None => return,
                },
                _ = reload.tick() => self.reload().await,
                _ = self.alerts.wait_for_change() => self.reload().await,
                _ = sweep.tick() => self.alerts.sweep(TimestampMicros::now()),
            }
        }
    }

    async fn reload(&self) {
        match self.rules.list_enabled().await {
            Ok(rules) => self.alerts.reload(rules),
            Err(error) => tracing::warn!(%error, "real-time alert rules reload failed"),
        }
    }

    async fn execute(&self, command: Command) {
        let outcome = match command {
            Command::Fire {
                rule,
                count,
                severity,
                at,
            } => self
                .evaluator
                .fire_realtime(&rule, count, severity, at)
                .await
                .map_err(|error| (rule.id.clone(), error)),
            Command::Clear { rule, at } => self
                .evaluator
                .clear_realtime(&rule, at)
                .await
                .map_err(|error| (rule.id.clone(), error)),
        };

        if let Err((rule_id, error)) = outcome {
            tracing::warn!(%rule_id, %error, "real-time alert action failed");
        }
    }
}
