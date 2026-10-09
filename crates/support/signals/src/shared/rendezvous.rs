// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Highest-random-weight（rendezvous）哈希。
//!
//! 给定 key 与候选节点，持有同一份候选集合的进程选出同一个 owner；节点增减只迁移该节点
//! 名下的 key，其余 key 的 owner 不变。Trace 尾采样与后台工作归属共用这一个实现。

/// `(key, node_id)` 的权重；owner 是权重最大的节点。
pub fn weight(key: &str, node_id: &str) -> u128 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(key.as_bytes());
    hasher.update(&[0]);
    hasher.update(node_id.as_bytes());
    u128::from_be_bytes(hasher.finalize().as_bytes()[..16].try_into().unwrap())
}

/// 从候选节点 ID 中选出 `key` 的 owner；候选为空时返回 `None`。
pub fn owner<'a>(key: &str, node_ids: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    node_ids
        .into_iter()
        .max_by_key(|node_id| weight(key, node_id))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn keys() -> Vec<String> {
        (0..1_000).map(|index| format!("key-{index}")).collect()
    }

    #[test]
    fn no_candidates_means_no_owner() {
        assert_eq!(owner("key", []), None);
    }

    #[test]
    fn owner_does_not_depend_on_candidate_order() {
        for key in keys() {
            assert_eq!(
                owner(&key, ["a", "b", "c"]),
                owner(&key, ["c", "a", "b"]),
                "{key}"
            );
        }
    }

    #[test]
    fn keys_spread_across_every_candidate() {
        let mut owned = HashMap::<&str, usize>::new();
        for key in keys() {
            *owned
                .entry(owner(&key, ["a", "b", "c"]).unwrap())
                .or_default() += 1;
        }
        assert_eq!(owned.len(), 3);
        // 1000 个 key 均分给 3 个节点时每个约 333；远离 0 即说明没有节点被饿死。
        assert!(owned.values().all(|count| *count > 200), "{owned:?}");
    }

    #[test]
    fn removing_a_candidate_only_moves_its_own_keys() {
        for key in keys() {
            let before = owner(&key, ["a", "b", "c"]).unwrap();
            let after = owner(&key, ["a", "c"]).unwrap();
            if before != "b" {
                assert_eq!(before, after, "{key}");
            }
        }
    }

    #[test]
    fn adding_a_candidate_only_takes_keys_for_itself() {
        for key in keys() {
            let before = owner(&key, ["a", "b"]).unwrap();
            let after = owner(&key, ["a", "b", "c"]).unwrap();
            assert!(after == before || after == "c", "{key}");
        }
    }
}
