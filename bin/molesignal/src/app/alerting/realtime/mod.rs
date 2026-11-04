// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 实时规则：写入路径上的计数器。
//!
//! `kind = real_time` 的规则在记录写入时逐条求值，而不是按周期查询。语句形态与窗口语义见
//! [`spec`]：统计最近 `period_secs` 秒内写入某个数据流且满足条件的记录数。
//!
//! 数据流程：
//! 1. [`RealtimeAlerts::observe`]：写入前对留存的记录求值，得到每条规则命中的记录下标；
//! 2. [`RealtimeAlerts::record`]：写入成功后把命中数计入规则在本节点的滑动窗口；
//! 3. [`RealtimeCoordinator`]：负责这条规则的 alert 节点每秒向各个接收写入的节点拉窗口计数
//!    （[`RealtimeAlerts::counts`]）并求和，满足触发条件就开事件，不再满足就解决。
//!
//! 开事件与解决复用周期评估器的事件流程（指纹去重、静默期、升级策略与通知）。计数在各节点
//! 内存里，按写入该节点的记录统计，每 5 秒存一份快照（[`worker`]）：节点重启时读回，节点不在了
//! 由负责汇总的节点用它的快照顶上。规则设了汇总集群（`query.home_cluster`）时，只有那个集群开事件，
//! 它把联邦里各集群的计数加起来（[`FederatedCounts`]）。

mod coordinator;
mod predicate;
mod spec;
mod window;
mod worker;

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

pub use coordinator::{FederatedCounts, RealtimeCoordinator, RealtimeCounts};
use parking_lot::{Mutex, RwLock};
use spec::RealtimeSpec;
pub use spec::{MAX_WINDOW_SECS, is_matched_at_intake, validate};
use tokio::sync::Notify;
use window::SlidingCounter;
pub use worker::RealtimeAlertWorker;

use crate::{
    domain::{
        alerting::{
            realtime_snapshot::CountSnapshot,
            rule::{AlertRule, AlertRuleKind},
        },
        intake::{IntakeError, RawEvent},
        stream::StreamType,
    },
    shared::{ids::Id, time::TimestampMicros},
};

/// 集群内部调用的来源标记头；拉计数时取 [`REALTIME_COUNTS_ORIGIN`]。
pub const INTERNAL_ORIGIN_HEADER: &str = "x-molesignal-internal-origin";
pub const REALTIME_COUNTS_ORIGIN: &str = "realtime-counts";

type Key = (Id, StreamType, String);

/// 规则的运行时：编译结果与窗口计数。规则重新加载时，语句与触发条件没有变化的规则沿用原有
/// 运行时，窗口计数不会因重新加载而清零。
struct Runtime {
    spec: RealtimeSpec,
    window: Mutex<SlidingCounter>,
}

#[derive(Clone)]
struct Entry {
    rule: Arc<AlertRule>,
    runtime: Arc<Runtime>,
}

/// 一批记录命中的规则及命中的记录下标。
pub struct PendingHits {
    hits: Vec<(Entry, Vec<usize>)>,
}

#[derive(Default)]
pub struct RealtimeAlerts {
    entries: RwLock<Arc<HashMap<Key, Vec<Entry>>>>,
    changed: Notify,
    /// 上一次加载时无法编译的规则；同一条规则只在第一次出现时告警。
    invalid: Mutex<HashSet<Id>>,
}

impl RealtimeAlerts {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// 规则被创建、修改或删除：唤醒后台任务立即重新加载。
    pub fn rules_changed(&self) {
        self.changed.notify_one();
    }

    /// 对一批将要写入的记录求值；该数据流没有规则命中任何记录时返回 `None`。
    pub fn observe(
        &self,
        org_id: &Id,
        stream: &str,
        stream_type: StreamType,
        events: &[RawEvent],
    ) -> Option<PendingHits> {
        let entries = self.entries.read().clone();
        let rules = entries.get(&(org_id.clone(), stream_type, stream.to_owned()))?;

        let hits: Vec<_> = rules
            .iter()
            .filter_map(|entry| {
                let indexes: Vec<usize> = events
                    .iter()
                    .enumerate()
                    .filter(|(_, event)| entry.runtime.spec.matches(&event.fields))
                    .map(|(index, _)| index)
                    .collect();

                (!indexes.is_empty()).then(|| (entry.clone(), indexes))
            })
            .collect();

        (!hits.is_empty()).then_some(PendingHits { hits })
    }

    /// 写入完成后记账。`rejected` 是写入端拒收的记录，不计入窗口。
    pub fn record(&self, pending: PendingHits, rejected: &[IntakeError], now: TimestampMicros) {
        let rejected: HashSet<usize> = rejected.iter().map(|error| error.index).collect();
        let second = now.0.div_euclid(1_000_000);

        for (entry, indexes) in pending.hits {
            let accepted = indexes
                .iter()
                .filter(|index| !rejected.contains(index))
                .count() as u64;

            if accepted > 0 {
                entry.runtime.window.lock().add(second, accepted);
            }
        }
    }

    /// 本节点上这几条规则窗口内的命中数，到 `now` 这一秒为止；本节点没加载的规则不返回。
    pub fn counts(&self, rule_ids: &[Id], now: TimestampMicros) -> Vec<(Id, u64)> {
        let wanted: HashSet<&Id> = rule_ids.iter().collect();
        let second = now.0.div_euclid(1_000_000);
        let entries = self.entries.read().clone();

        entries
            .values()
            .flatten()
            .filter(|entry| wanted.contains(&entry.rule.id))
            .map(|entry| {
                let count = entry
                    .runtime
                    .window
                    .lock()
                    .count(second, entry.runtime.spec.window_secs);

                (entry.rule.id.clone(), count)
            })
            .collect()
    }

    /// 本节点窗口里还有计数的规则的快照。
    pub fn snapshots(&self, node_id: &str, now: TimestampMicros) -> Vec<CountSnapshot> {
        let second = now.0.div_euclid(1_000_000);
        let entries = self.entries.read().clone();

        entries
            .values()
            .flatten()
            .filter_map(|entry| {
                let mut window = entry.runtime.window.lock();

                if window.count(second, entry.runtime.spec.window_secs) == 0 {
                    return None;
                }

                Some(CountSnapshot {
                    node_id: node_id.to_string(),
                    rule_id: entry.rule.id.clone(),
                    window_secs: entry.runtime.spec.window_secs,
                    buckets: window.buckets(),
                    saved_at: now,
                })
            })
            .collect()
    }

    /// 读回重启前存的快照：还加载着的规则把格子并进窗口。返回并进了几条规则。
    pub fn restore(&self, snapshots: &[CountSnapshot]) -> usize {
        let entries = self.entries.read().clone();
        let mut restored = 0;

        for entry in entries.values().flatten() {
            for snapshot in snapshots
                .iter()
                .filter(|snapshot| snapshot.rule_id == entry.rule.id)
            {
                entry.runtime.window.lock().add_buckets(&snapshot.buckets);
                restored += 1;
            }
        }

        restored
    }

    /// 用最新的规则集合替换匹配表。
    pub(super) fn reload(&self, rules: Vec<AlertRule>) {
        let previous = self.entries.read().clone();
        let mut next: HashMap<Key, Vec<Entry>> = HashMap::new();
        let mut invalid = HashSet::new();
        let mut warned = self.invalid.lock();

        for rule in rules
            .into_iter()
            .filter(|rule| rule.enabled && rule.kind == AlertRuleKind::RealTime)
        {
            let spec = match RealtimeSpec::from_rule(&rule) {
                Ok(spec) => spec,
                Err(reason) => {
                    // 无法在写入路径上运行的规则交给周期评估器。
                    if !warned.contains(&rule.id) {
                        tracing::warn!(
                            rule_id = %rule.id,
                            %reason,
                            "real_time rule cannot run at intake; the scheduled evaluator keeps it"
                        );
                    }

                    invalid.insert(rule.id.clone());
                    continue;
                }
            };

            let key = (rule.org_id.clone(), spec.stream_type, spec.stream.clone());

            let runtime = previous
                .get(&key)
                .and_then(|entries| {
                    entries
                        .iter()
                        .find(|entry| entry.rule.id == rule.id && entry.runtime.spec == spec)
                })
                .map(|entry| entry.runtime.clone())
                .unwrap_or_else(|| {
                    Arc::new(Runtime {
                        spec,
                        window: Mutex::new(SlidingCounter::default()),
                    })
                });

            next.entry(key).or_default().push(Entry {
                rule: Arc::new(rule),
                runtime,
            });
        }

        *warned = invalid;
        drop(warned);
        *self.entries.write() = Arc::new(next);
    }

    pub(super) async fn wait_for_change(&self) {
        self.changed.notified().await;
    }
}

#[cfg(test)]
mod tests;
