// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 实时规则的汇总判断：接收写入的节点只按秒计数，不各自开事件；负责这条规则的 alert 节点（按规则
//! id 归属）每秒向它们拉一次窗口内的命中数并求和，满足触发条件就开事件、升档就升级严重度、不再
//! 满足就解决。单机时参与的只有自己，结果和逐批判断一样，只是最多晚一秒。
//!
//! - 接手一条规则时按它进行中的事件接上状态：有事件就是已触发，事件的严重度就是触发的那一档。
//! - 某个节点拉不到时用它最近的快照顶上；计数在接收写入的节点上，负责汇总的节点换了不丢。
//! - 规则设了汇总集群（`query.home_cluster`）时只在那个集群判断：汇总集群加上联邦里别的集群的
//!   计数（[`FederatedCounts`]），别的集群只计数。本集群没开联邦时照常判断。

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use futures::future::join_all;
use tokio::task::JoinHandle;

use super::spec::{Level, RealtimeSpec};
use crate::{
    app::{alerting::RuleEvaluator, cluster::WorkOwnership},
    domain::alerting::{
        incident::Severity,
        repositories::AlertRuleRepository,
        rule::{AlertRule, AlertRuleKind},
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// 汇总一次的间隔。
const TICK_EVERY: Duration = Duration::from_secs(1);

/// 重新读规则的间隔：别的节点上的规则改动在这个时间内生效。
const RELOAD_EVERY: Duration = Duration::from_secs(10);

/// 归属指标里这类工作的名字。
const WORK: &str = "realtime_rules";

/// 各接收写入的节点上，这些规则窗口内的命中数之和。拉不到的节点不算。
#[async_trait]
pub trait RealtimeCounts: Send + Sync {
    async fn counts(&self, rule_ids: &[Id], now: TimestampMicros) -> HashMap<Id, u64>;
}

/// 联邦里别的集群上这些规则的计数（那个集群各节点之和）：汇总集群是本集群的规则加上它。
#[async_trait]
pub trait FederatedCounts: Send + Sync {
    /// 本集群在联邦里的 id；没开联邦时为空。
    async fn local_cluster(&self) -> Option<String>;
    /// 映射了这个组织的各集群上这些规则的计数之和。拉不到的集群不算。
    async fn remote_counts(&self, org_id: &Id, rule_ids: &[Id]) -> HashMap<Id, u64>;
}

/// 规则在本集群判断：没设汇总集群、汇总集群就是本集群，或本集群没开联邦。
pub(super) fn decided_here(home_cluster: Option<&str>, local_cluster: Option<&str>) -> bool {
    match (home_cluster, local_cluster) {
        (Some(home), Some(local)) => home == local,
        _ => true,
    }
}

/// 一条规则在汇总节点上的状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct Standing {
    pub(super) firing: bool,
    /// 已触发的最高一档；单档规则为 `None`。
    pub(super) severity: Option<Severity>,
}

/// 这一秒要做的事。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    Fire { severity: Option<Severity> },
    Clear,
}

/// 按这一秒的档位决定开事件、升档还是解决，并改好状态。
pub(super) fn decide(standing: &mut Standing, level: Level) -> Option<Step> {
    match level {
        Some(severity) if !standing.firing || severity > standing.severity => {
            standing.firing = true;
            standing.severity = severity;
            Some(Step::Fire { severity })
        }
        Some(_) => None,
        None if standing.firing => {
            *standing = Standing::default();
            Some(Step::Clear)
        }
        None => None,
    }
}

struct Tracked {
    rule: Arc<AlertRule>,
    spec: RealtimeSpec,
    /// 还没按进行中的事件接上状态时为空。
    standing: Option<Standing>,
}

pub struct RealtimeCoordinator {
    rules: Arc<dyn AlertRuleRepository>,
    evaluator: Arc<RuleEvaluator>,
    ownership: Arc<WorkOwnership>,
    counts: Arc<dyn RealtimeCounts>,
    federation: Option<Arc<dyn FederatedCounts>>,
}

impl RealtimeCoordinator {
    pub fn new(
        rules: Arc<dyn AlertRuleRepository>,
        evaluator: Arc<RuleEvaluator>,
        ownership: Arc<WorkOwnership>,
        counts: Arc<dyn RealtimeCounts>,
    ) -> Self {
        Self {
            rules,
            evaluator,
            ownership,
            counts,
            federation: None,
        }
    }

    /// 开了联邦：设了汇总集群的规则只在汇总集群判断，加上别的集群的计数。
    pub fn with_federation(mut self, federation: Arc<dyn FederatedCounts>) -> Self {
        self.federation = Some(federation);
        self
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(self.run())
    }

    #[tracing::instrument(
        name = "worker.realtime_coordinator",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "realtime_coordinator")
    )]
    async fn run(self) {
        let mut tracked: HashMap<Id, Tracked> = HashMap::new();
        let mut tick = tokio::time::interval(TICK_EVERY);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut reload = tokio::time::interval(RELOAD_EVERY);

        loop {
            tokio::select! {
                _ = reload.tick() => self.reload(&mut tracked).await,
                _ = tick.tick() => self.round(&mut tracked, TimestampMicros::now()).await,
            }
        }
    }

    /// 换成最新的规则：语句与触发条件没变的规则留着状态。
    async fn reload(&self, tracked: &mut HashMap<Id, Tracked>) {
        let rules = match self.rules.list_enabled().await {
            Ok(rules) => rules,
            Err(error) => {
                tracing::warn!(%error, "real-time rules could not be read for the cluster count");
                return;
            }
        };

        let mut next = HashMap::new();

        for rule in rules
            .into_iter()
            .filter(|rule| rule.enabled && rule.kind == AlertRuleKind::RealTime)
        {
            // 写入路径上跑不了的规则由周期评估器按查询评估。
            let Ok(spec) = RealtimeSpec::from_rule(&rule) else {
                continue;
            };

            let standing = tracked
                .remove(&rule.id)
                .and_then(|previous| previous.standing);

            next.insert(
                rule.id.clone(),
                Tracked {
                    rule: Arc::new(rule),
                    spec,
                    standing,
                },
            );
        }

        *tracked = next;
    }

    /// 汇总一次本节点负责的规则。
    async fn round(&self, tracked: &mut HashMap<Id, Tracked>, now: TimestampMicros) {
        // 一轮读一次成员视图：每条规则各读一次会每秒多出许多次注册表查询。
        let view = self.ownership.view().await;
        let local_cluster = match &self.federation {
            Some(federation) => federation.local_cluster().await,
            None => None,
        };

        let owned: HashSet<Id> = tracked
            .iter()
            .filter(|(id, entry)| {
                view.owns(id.as_str())
                    && decided_here(
                        entry.rule.query.home_cluster.as_deref(),
                        local_cluster.as_deref(),
                    )
            })
            .map(|(id, _)| id.clone())
            .collect();

        self.ownership.record_owned(WORK, owned.len());

        // 让出去的规则忘掉状态：再接手时按进行中的事件重新接上。
        for (id, entry) in tracked.iter_mut() {
            if !owned.contains(id) {
                entry.standing = None;
            }
        }

        if owned.is_empty() {
            return;
        }

        let ids: Vec<Id> = owned.iter().cloned().collect();
        let mut counts = self.counts.counts(&ids, now).await;

        if let Some(local) = local_cluster.as_deref() {
            self.add_remote(&mut counts, tracked, &ids, local).await;
        }

        for id in ids {
            let Some(entry) = tracked.get_mut(&id) else {
                continue;
            };

            let count = counts.get(&id).copied().unwrap_or_default();

            if let Err(error) = self.settle(entry, count, now).await {
                tracing::warn!(rule_id = %id, %error, "real-time rule could not be settled");
            }
        }
    }

    /// 汇总集群是本集群的规则加上联邦里别的集群的计数，各组织同时拉。
    async fn add_remote(
        &self,
        counts: &mut HashMap<Id, u64>,
        tracked: &HashMap<Id, Tracked>,
        ids: &[Id],
        local: &str,
    ) {
        let Some(federation) = &self.federation else {
            return;
        };

        let mut by_org: HashMap<&Id, Vec<Id>> = HashMap::new();

        for id in ids {
            if let Some(entry) = tracked.get(id)
                && entry.rule.query.home_cluster.as_deref() == Some(local)
            {
                by_org
                    .entry(&entry.rule.org_id)
                    .or_default()
                    .push(id.clone());
            }
        }

        let replies = join_all(
            by_org
                .iter()
                .map(|(org_id, rule_ids)| federation.remote_counts(org_id, rule_ids)),
        )
        .await;

        for (id, count) in replies.into_iter().flatten() {
            *counts.entry(id).or_default() += count;
        }
    }

    async fn settle(&self, entry: &mut Tracked, count: u64, now: TimestampMicros) -> Result<()> {
        let mut standing = match entry.standing {
            Some(standing) => standing,
            None => self.picked_up(&entry.rule).await?,
        };

        let step = decide(&mut standing, entry.spec.level(count));

        match step {
            Some(Step::Fire { severity }) => {
                self.evaluator
                    .fire_realtime(&entry.rule, count, severity, now)
                    .await?;
            }
            Some(Step::Clear) => self.evaluator.clear_realtime(&entry.rule, now).await?,
            None => {}
        }

        entry.standing = Some(standing);
        Ok(())
    }

    /// 刚接手：有进行中的事件就是已触发。多档规则的档位取事件的严重度。
    async fn picked_up(&self, rule: &AlertRule) -> Result<Standing> {
        let open = self.evaluator.realtime_open(rule).await?;

        Ok(Standing {
            firing: open.is_some(),
            severity: open.filter(|_| !rule.thresholds.is_empty()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fires_once_raises_on_a_higher_tier_and_clears_below_the_trigger() {
        let mut standing = Standing::default();

        assert_eq!(decide(&mut standing, None), None);
        assert_eq!(
            decide(&mut standing, Some(Some(Severity::Warning))),
            Some(Step::Fire {
                severity: Some(Severity::Warning)
            })
        );
        // 同一档、更低一档都不再开。
        assert_eq!(decide(&mut standing, Some(Some(Severity::Warning))), None);
        assert_eq!(decide(&mut standing, Some(Some(Severity::Info))), None);
        assert_eq!(
            decide(&mut standing, Some(Some(Severity::Critical))),
            Some(Step::Fire {
                severity: Some(Severity::Critical)
            })
        );
        assert_eq!(decide(&mut standing, None), Some(Step::Clear));
        assert_eq!(standing, Standing::default());
        assert_eq!(decide(&mut standing, None), None);
    }

    #[test]
    fn a_single_tier_rule_fires_once_and_a_picked_up_one_does_not_fire_again() {
        let mut fresh = Standing::default();
        assert_eq!(
            decide(&mut fresh, Some(None)),
            Some(Step::Fire { severity: None })
        );
        assert_eq!(decide(&mut fresh, Some(None)), None);

        // 接手时已经有进行中的事件：照样满足条件就什么也不做，不满足就解决。
        let mut picked_up = Standing {
            firing: true,
            severity: None,
        };
        assert_eq!(decide(&mut picked_up, Some(None)), None);
        assert_eq!(decide(&mut picked_up, None), Some(Step::Clear));
    }

    #[test]
    fn a_rule_with_a_home_cluster_is_decided_only_there() {
        assert!(decided_here(None, Some("east")));
        assert!(decided_here(Some("east"), Some("east")));
        assert!(!decided_here(Some("east"), Some("west")));

        // 本集群没开联邦：汇总集群不起作用，照常判断。
        assert!(decided_here(Some("east"), None));
        assert!(decided_here(None, None));
    }
}
