// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Real-time alert matcher cache + IngestService 集成。
//!
//! - [`RealtimeMatcherCache`]：`tokio::sync::watch` 推 `Arc<HashMap<(org, stream_type, stream), Vec<CompiledRule>>>`。
//!   alert_manager 启动时 init；rule CRUD 后调 `reload`。
//! - `IngestService::ingest` 在 WAL append 之后调 [`RealtimeMatcherCache::matches`]
//!   对每条 record 跑判定；命中 emit `IncidentEvent` 到 broadcast channel。
//!
//! 当前简化：matcher 只支持 `field == value` 的 equality 形态；正则 / SQL WHERE 留扩展位。

use std::{collections::HashMap, sync::Arc};

use parking_lot::RwLock;
use tokio::sync::broadcast;

use crate::{
    domain::{ingestion::RawEvent, stream::StreamType},
    shared::{ids::Id, time::TimestampMicros},
};

pub type RealtimeKey = (Id, StreamType, String);

#[derive(Debug, Clone)]
pub struct CompiledRule {
    pub rule_id: Id,
    pub field: String,
    pub value: serde_json::Value,
}

impl CompiledRule {
    pub fn matches(&self, ev: &RawEvent) -> bool {
        ev.fields.get(&self.field) == Some(&self.value)
    }
}

#[derive(Debug, Clone)]
pub struct IncidentEvent {
    pub rule_id: Id,
    pub org_id: Id,
    pub stream: String,
    pub stream_type: StreamType,
    pub fired_at: TimestampMicros,
}

pub struct RealtimeMatcherCache {
    map: RwLock<Arc<HashMap<RealtimeKey, Vec<CompiledRule>>>>,
    events_tx: broadcast::Sender<IncidentEvent>,
}

impl Default for RealtimeMatcherCache {
    fn default() -> Self {
        Self::new()
    }
}

impl RealtimeMatcherCache {
    pub fn new() -> Self {
        let (events_tx, _) = broadcast::channel(1024);
        Self {
            map: RwLock::new(Arc::new(HashMap::new())),
            events_tx,
        }
    }

    pub fn snapshot(&self) -> Arc<HashMap<RealtimeKey, Vec<CompiledRule>>> {
        self.map.read().clone()
    }

    pub fn reload(&self, new_map: HashMap<RealtimeKey, Vec<CompiledRule>>) {
        *self.map.write() = Arc::new(new_map);
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<IncidentEvent> {
        self.events_tx.subscribe()
    }

    /// 对一条 record 跑所有 (org, stream) 对应的 CompiledRule；命中即 emit IncidentEvent。
    /// 返回命中数量。
    pub fn matches(
        &self,
        org_id: &Id,
        stream: &str,
        stream_type: StreamType,
        ev: &RawEvent,
    ) -> usize {
        let map = self.snapshot();
        let key = (org_id.clone(), stream_type, stream.to_string());
        let rules = match map.get(&key) {
            Some(r) => r,
            None => return 0,
        };
        let mut hits = 0;
        for rule in rules {
            if rule.matches(ev) {
                let _ = self.events_tx.send(IncidentEvent {
                    rule_id: rule.rule_id.clone(),
                    org_id: org_id.clone(),
                    stream: stream.to_string(),
                    stream_type,
                    fired_at: TimestampMicros::now(),
                });
                hits += 1;
            }
        }
        hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(field: &str, value: serde_json::Value) -> RawEvent {
        let mut f = serde_json::Map::new();
        f.insert(field.into(), value);
        RawEvent {
            timestamp: TimestampMicros::now(),
            fields: f,
        }
    }

    #[tokio::test]
    async fn match_emits_incident_event() {
        let cache = RealtimeMatcherCache::new();
        let mut rx = cache.subscribe_events();
        let mut map = HashMap::new();
        map.insert(
            (Id::from_string("orga"), StreamType::Logs, "app".into()),
            vec![CompiledRule {
                rule_id: Id::from_string("r1"),
                field: "level".into(),
                value: serde_json::json!("fatal"),
            }],
        );
        cache.reload(map);

        let hits = cache.matches(
            &Id::from_string("orga"),
            "app",
            StreamType::Logs,
            &ev("level", serde_json::json!("fatal")),
        );
        assert_eq!(hits, 1);
        let received = rx.recv().await.unwrap();
        assert_eq!(received.rule_id.0, "r1");
    }

    #[tokio::test]
    async fn no_match_when_value_differs() {
        let cache = RealtimeMatcherCache::new();
        let mut map = HashMap::new();
        map.insert(
            (Id::from_string("orga"), StreamType::Logs, "app".into()),
            vec![CompiledRule {
                rule_id: Id::from_string("r1"),
                field: "level".into(),
                value: serde_json::json!("fatal"),
            }],
        );
        cache.reload(map);
        let hits = cache.matches(
            &Id::from_string("orga"),
            "app",
            StreamType::Logs,
            &ev("level", serde_json::json!("info")),
        );
        assert_eq!(hits, 0);
    }
}
