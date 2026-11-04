// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 评估延迟：规则查询的窗口往回挪多少秒，等晚到或还没刷写的数据。规则自己设的优先；没设时，一个
//! 进程跑全部角色的单机读得到自己缓冲里的数据，不挪；分角色集群用实例设置里的默认值
//! （[`EvaluationDelay`]），各告警节点每分钟读一次。

use std::{
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};

use tokio::task::JoinHandle;

use super::RuleEvaluator;
use crate::domain::{
    alerting::rule::{AlertRule, EvaluationDelay},
    iam::{DEFAULT_ALERT_EVALUATION_DELAY_SECS, InstanceSettingsRepository},
};

/// 多久读一次实例设置。
const REFRESH_EVERY: Duration = Duration::from_secs(60);

pub struct DelaySettings {
    reads_own_buffer: bool,
    cluster_default_secs: AtomicU32,
}

impl DelaySettings {
    pub fn new(reads_own_buffer: bool) -> Arc<Self> {
        Arc::new(Self {
            reads_own_buffer,
            cluster_default_secs: AtomicU32::new(DEFAULT_ALERT_EVALUATION_DELAY_SECS),
        })
    }

    pub fn current(&self) -> EvaluationDelay {
        EvaluationDelay {
            reads_own_buffer: self.reads_own_buffer,
            cluster_default_secs: self.cluster_default_secs.load(Ordering::Relaxed),
        }
    }

    /// 每分钟从实例设置读一次分角色集群的默认值；读不出来时沿用上一次的。
    pub fn spawn_refresh(
        self: &Arc<Self>,
        settings: Arc<dyn InstanceSettingsRepository>,
    ) -> JoinHandle<()> {
        let delay = self.clone();

        tokio::spawn(async move {
            let mut tick = tokio::time::interval(REFRESH_EVERY);

            loop {
                tick.tick().await;

                match settings.get().await {
                    Ok(current) => delay
                        .cluster_default_secs
                        .store(current.alert_evaluation_delay_secs, Ordering::Relaxed),
                    Err(error) => {
                        tracing::debug!(%error, "alert evaluation delay unreadable; keeping the last one");
                    }
                }
            }
        })
    }
}

impl RuleEvaluator {
    /// 这条规则的查询窗口往回挪多少微秒。
    pub(super) fn delay_micros(&self, rule: &AlertRule) -> i64 {
        i64::from(self.delay.current().for_query(&rule.query)) * 1_000_000
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::alerting::rule::AlertQuery;

    fn query(delay_secs: Option<u32>) -> AlertQuery {
        serde_json::from_value(serde_json::json!({
            "language": "sql",
            "statement": "SELECT count(*) FROM logs",
            "periodSecs": 300,
            "delaySecs": delay_secs,
        }))
        .unwrap()
    }

    #[test]
    fn a_rule_waits_as_long_as_it_says_or_as_the_deployment_needs() {
        let standalone = DelaySettings::new(true);
        let cluster = DelaySettings::new(false);
        cluster.cluster_default_secs.store(45, Ordering::Relaxed);

        assert_eq!(standalone.current().for_query(&query(None)), 0);
        assert_eq!(cluster.current().for_query(&query(None)), 45);
        assert_eq!(cluster.current().for_query(&query(Some(300))), 300);
        assert_eq!(standalone.current().for_query(&query(Some(0))), 0);
    }
}
