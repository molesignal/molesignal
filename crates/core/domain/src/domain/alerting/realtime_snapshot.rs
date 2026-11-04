// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 实时规则计数的快照：每个接收写入的节点每隔几秒把它窗口里的计数（每秒一格）存一份。
//!
//! - 节点重启时读回自己的那份，计数接着算，不从零开始；
//! - 节点不在了（成员视图里没有它），负责汇总的节点用它快照里还在窗口内的格子顶上它那一份，直到
//!   这些格子出了窗口。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::shared::{Result, ids::Id, time::TimestampMicros};

/// 一个节点上一条规则的窗口计数。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CountSnapshot {
    pub node_id: String,
    pub rule_id: Id,
    /// 存的时候规则的窗口有多长：算还在窗口内的格子用。
    pub window_secs: u32,
    /// `(秒, 这一秒的命中数)`，秒从小到大。
    pub buckets: Vec<(i64, u64)>,
    pub saved_at: TimestampMicros,
}

impl CountSnapshot {
    /// 到 `now` 这一秒为止、还在窗口里的命中数。
    pub fn count_at(&self, now: TimestampMicros) -> u64 {
        let second = now.0.div_euclid(1_000_000);
        let oldest_excluded = second - i64::from(self.window_secs);

        self.buckets
            .iter()
            .filter(|(at, _)| *at > oldest_excluded && *at <= second)
            .map(|(_, count)| count)
            .sum()
    }
}

#[async_trait]
pub trait RealtimeSnapshotRepository: Send + Sync {
    /// 换掉这个节点的全部快照：没在这一批里的规则（窗口已经空了）删掉。
    async fn replace_node(&self, node_id: &str, snapshots: &[CountSnapshot]) -> Result<()>;
    async fn load_node(&self, node_id: &str) -> Result<Vec<CountSnapshot>>;
    /// 这些规则在各节点的快照。
    async fn load_rules(&self, rule_ids: &[Id]) -> Result<Vec<CountSnapshot>>;
    /// 删掉 `before` 之前存的；返回删了几条。
    async fn delete_before(&self, before: TimestampMicros) -> Result<u64>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snapshot_counts_only_what_is_still_in_the_window() {
        let snapshot = CountSnapshot {
            node_id: "a".into(),
            rule_id: Id::from_string("r"),
            window_secs: 10,
            buckets: vec![(100, 3), (105, 2), (109, 4)],
            saved_at: TimestampMicros(109_000_000),
        };

        assert_eq!(snapshot.count_at(TimestampMicros(109_000_000)), 9);
        assert_eq!(snapshot.count_at(TimestampMicros(112_500_000)), 6);
        assert_eq!(snapshot.count_at(TimestampMicros(119_000_000)), 0);
    }
}
