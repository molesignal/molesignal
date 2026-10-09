// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 实时规则：写入路径上的匹配器。
//!
//! `kind = real_time` 的规则在记录写入时逐条求值，而不是按周期查询。语句形态与窗口语义见
//! [`spec`]：统计最近 `period_secs` 秒内写入某个数据流且满足条件的记录数，窗口内的命中数
//! 满足触发条件时立即开事件，不再满足时自动解决。
//!
//! 数据流程：
//! 1. [`RealtimeAlerts::observe`]：写入前对留存的记录求值，得到每条规则命中的记录下标；
//! 2. [`RealtimeAlerts::record`]：写入成功后把命中数计入规则的滑动窗口，窗口计数首次满足
//!    触发条件（或满足更高一档）时向 [`RealtimeAlertWorker`] 发出开事件命令；
//! 3. 后台任务周期性检查已触发的规则，窗口计数不再满足时发出解决命令，并重新加载规则。
//!
//! 开事件与解决复用周期评估器的事件流程（指纹去重、静默期、升级策略与通知）。计数在节点
//! 内存里，按写入该节点的记录统计。

mod predicate;
mod spec;
mod window;
mod worker;

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use parking_lot::{Mutex, RwLock};
use spec::RealtimeSpec;
pub use spec::{MAX_WINDOW_SECS, is_matched_at_intake, validate};
use tokio::sync::{Notify, mpsc};
use window::SlidingCounter;
pub use worker::RealtimeAlertWorker;

use crate::{
    domain::{
        alerting::{
            incident::Severity,
            rule::{AlertRule, AlertRuleKind},
        },
        intake::{IntakeError, RawEvent},
        stream::StreamType,
    },
    shared::{ids::Id, time::TimestampMicros},
};

/// 待执行命令的队列容量；队列满时丢弃命令并回退本地状态，下一批命中会重新触发。
const COMMAND_QUEUE: usize = 1024;

type Key = (Id, StreamType, String);

/// 规则的运行时：编译结果与窗口状态。规则重新加载时，语句与触发条件没有变化的规则沿用原有
/// 运行时，窗口计数不会因重新加载而清零。
struct Runtime {
    spec: RealtimeSpec,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    window: SlidingCounter,
    firing: bool,
    /// 已触发的最高一档；单档规则为 `None`。
    severity: Option<Severity>,
}

#[derive(Clone)]
struct Entry {
    rule: Arc<AlertRule>,
    runtime: Arc<Runtime>,
}

/// 发给后台任务的命令。
pub(super) enum Command {
    Fire {
        rule: Arc<AlertRule>,
        count: u64,
        severity: Option<Severity>,
        at: TimestampMicros,
    },
    Clear {
        rule: Arc<AlertRule>,
        at: TimestampMicros,
    },
}

/// 一批记录命中的规则及命中的记录下标。
pub struct PendingHits {
    hits: Vec<(Entry, Vec<usize>)>,
}

pub struct RealtimeAlerts {
    entries: RwLock<Arc<HashMap<Key, Vec<Entry>>>>,
    commands: mpsc::Sender<Command>,
    inbox: Mutex<Option<mpsc::Receiver<Command>>>,
    changed: Notify,
    /// 上一次加载时无法编译的规则；同一条规则只在第一次出现时告警。
    invalid: Mutex<HashSet<Id>>,
}

impl Default for RealtimeAlerts {
    fn default() -> Self {
        let (commands, inbox) = mpsc::channel(COMMAND_QUEUE);
        Self {
            entries: RwLock::new(Arc::new(HashMap::new())),
            commands,
            inbox: Mutex::new(Some(inbox)),
            changed: Notify::new(),
            invalid: Mutex::new(HashSet::new()),
        }
    }
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
            if accepted == 0 {
                continue;
            }
            let mut state = entry.runtime.state.lock();
            state.window.add(second, accepted);
            let count = state.window.count(second, entry.runtime.spec.window_secs);
            let Some(level) = entry.runtime.spec.level(count) else {
                continue;
            };
            if state.firing && level <= state.severity {
                continue;
            }
            let sent = self.commands.try_send(Command::Fire {
                rule: entry.rule.clone(),
                count,
                severity: level,
                at: now,
            });
            match sent {
                Ok(()) => {
                    state.firing = true;
                    state.severity = level;
                }
                Err(_) => tracing::warn!(
                    rule_id = %entry.rule.id,
                    "real-time alert queue is full; the trigger is retried on the next match"
                ),
            }
        }
    }

    /// 已触发的规则：窗口计数不再满足触发条件时发出解决命令。
    pub(super) fn sweep(&self, now: TimestampMicros) {
        let second = now.0.div_euclid(1_000_000);
        let entries = self.entries.read().clone();
        for entry in entries.values().flatten() {
            let mut state = entry.runtime.state.lock();
            if !state.firing {
                continue;
            }
            let count = state.window.count(second, entry.runtime.spec.window_secs);
            if entry.runtime.spec.level(count).is_some() {
                continue;
            }
            let sent = self.commands.try_send(Command::Clear {
                rule: entry.rule.clone(),
                at: now,
            });
            if sent.is_ok() {
                state.firing = false;
                state.severity = None;
            }
        }
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
                        state: Mutex::new(State::default()),
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

    pub(super) fn take_inbox(&self) -> Option<mpsc::Receiver<Command>> {
        self.inbox.lock().take()
    }

    pub(super) async fn wait_for_change(&self) {
        self.changed.notified().await;
    }
}

#[cfg(test)]
mod tests;
