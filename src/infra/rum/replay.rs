// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! RUM session-replay segment persistence.
//!
//! Segments are validated, NDJSON encoded, compressed, and stored under a
//! tenant-scoped deterministic key. PostgreSQL metadata supplies ordering,
//! idempotency, availability filtering, quotas, and retention cleanup.

use std::{collections::HashSet, sync::Arc};

use async_trait::async_trait;
use bytes::Bytes;
use object_store::{ObjectStore, ObjectStoreExt, path::Path as ObjPath};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

mod repository;

pub use repository::PgRumReplayMetaRepository;

pub const MAX_REPLAY_EVENTS_PER_SEGMENT: usize = 500;
pub const MAX_REPLAY_SEGMENT_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_REPLAY_SESSION_BYTES: u64 = 100 * 1024 * 1024;
pub const MAX_REPLAY_SEGMENTS_PER_SESSION: usize = 2_000;
pub const MAX_REPLAY_FILTER_SESSION_IDS: usize = 10_000;

#[derive(Debug, Clone)]
pub struct RumReplayRecord {
    pub id: Id,
    pub org_id: Id,
    pub application_id: String,
    pub session_id: String,
    pub seq: i32,
    pub object_key: String,
    pub bytes_uncompressed: u64,
    pub event_count: usize,
    pub has_full_snapshot: bool,
    pub content_hash: String,
    pub first_event_at_micros: i64,
    pub created_at: TimestampMicros,
}

#[async_trait]
pub trait RumReplayMetaRepository: Send + Sync {
    async fn find_segment(
        &self,
        org_id: &Id,
        application_id: &str,
        session_id: &str,
        seq: i32,
    ) -> Result<Option<RumReplayRecord>>;
    async fn insert_if_absent(&self, row: &RumReplayRecord) -> Result<bool>;
    async fn list_for_session(&self, org_id: &Id, session_id: &str)
    -> Result<Vec<RumReplayRecord>>;
    async fn session_usage(
        &self,
        org_id: &Id,
        application_id: &str,
        session_id: &str,
    ) -> Result<(u64, usize)>;
    async fn existing_session_ids(
        &self,
        org_id: &Id,
        session_ids: &[String],
    ) -> Result<HashSet<String>>;
    async fn session_ids_in_window(
        &self,
        org_id: &Id,
        from_micros: i64,
        to_micros: i64,
        limit: usize,
    ) -> Result<Vec<String>>;
    async fn list_expired(&self, cutoff_micros: i64, limit: usize) -> Result<Vec<RumReplayRecord>>;
    async fn delete_ids(&self, ids: &[String]) -> Result<()>;
}

pub struct RumReplayWriter {
    object_store: Arc<dyn ObjectStore>,
    meta: Arc<dyn RumReplayMetaRepository>,
}

impl RumReplayWriter {
    pub fn new(object_store: Arc<dyn ObjectStore>, meta: Arc<dyn RumReplayMetaRepository>) -> Self {
        Self { object_store, meta }
    }

    pub async fn put_session_events(
        &self,
        org_id: &Id,
        application_id: &str,
        session_id: &str,
        seq: i32,
        events: &[Value],
    ) -> Result<RumReplayRecord> {
        validate_segment(session_id, seq, events)?;
        let first_event_at_micros = events
            .iter()
            .filter_map(event_timestamp_millis)
            .min()
            .and_then(|timestamp| i64::try_from(timestamp.saturating_mul(1_000)).ok())
            .ok_or_else(|| Error::invalid("RUM replay event timestamp is invalid"))?;
        let ndjson = encode_events(events)?;
        let bytes_uncompressed = ndjson.len() as u64;
        if bytes_uncompressed > MAX_REPLAY_SEGMENT_BYTES {
            return Err(Error::payload_too_large(format!(
                "RUM replay segment exceeds {} bytes",
                MAX_REPLAY_SEGMENT_BYTES
            )));
        }
        let content_hash = hex::encode(Sha256::digest(&ndjson));

        if let Some(existing) = self
            .meta
            .find_segment(org_id, application_id, session_id, seq)
            .await?
        {
            return same_segment(existing, &content_hash);
        }
        let (used, segment_count) = self
            .meta
            .session_usage(org_id, application_id, session_id)
            .await?;
        if segment_count >= MAX_REPLAY_SEGMENTS_PER_SESSION {
            return Err(Error::resource_exhausted(format!(
                "RUM replay session exceeds {MAX_REPLAY_SEGMENTS_PER_SESSION} segments"
            )));
        }
        if used.saturating_add(bytes_uncompressed) > MAX_REPLAY_SESSION_BYTES {
            return Err(Error::resource_exhausted(format!(
                "RUM replay session exceeds {} bytes",
                MAX_REPLAY_SESSION_BYTES
            )));
        }

        let hash_prefix = &content_hash[..16];
        let application_hash = hex::encode(Sha256::digest(application_id.as_bytes()));
        let object_key = format!(
            "{}/rum/{}/{}/{seq:010}-{hash_prefix}.ndjson.zst",
            org_id.0,
            &application_hash[..16],
            session_id
        );
        let path = ObjPath::parse(&object_key)
            .map_err(|error| Error::internal(format!("RUM replay object path: {error}")))?;
        let compressed =
            tokio::task::spawn_blocking(move || zstd::stream::encode_all(ndjson.as_slice(), 3))
                .await
                .map_err(|error| Error::internal(format!("RUM replay compression task: {error}")))?
                .map_err(|error| Error::internal(format!("RUM replay compression: {error}")))?;
        self.object_store
            .put(&path, Bytes::from(compressed).into())
            .await
            .map_err(|error| Error::internal(format!("RUM replay object write: {error}")))?;

        let record = RumReplayRecord {
            id: Id::new(),
            org_id: org_id.clone(),
            application_id: application_id.to_string(),
            session_id: session_id.to_string(),
            seq,
            object_key,
            bytes_uncompressed,
            event_count: events.len(),
            has_full_snapshot: events
                .iter()
                .any(|event| event.get("type").and_then(Value::as_u64) == Some(2)),
            content_hash,
            first_event_at_micros,
            created_at: TimestampMicros::now(),
        };
        if self.meta.insert_if_absent(&record).await? {
            return Ok(record);
        }

        let existing = self
            .meta
            .find_segment(org_id, application_id, session_id, seq)
            .await?
            .ok_or_else(|| Error::conflict("RUM replay segment insert raced with deletion"))?;
        if existing.content_hash != record.content_hash {
            let _ = self.object_store.delete(&path).await;
        }
        same_segment(existing, &record.content_hash)
    }

    pub async fn get_session_events(
        &self,
        org_id: &Id,
        session_id: &str,
    ) -> Result<(usize, Vec<Value>)> {
        validate_session_id(session_id)?;
        let records = self.meta.list_for_session(org_id, session_id).await?;
        if records.len() > MAX_REPLAY_SEGMENTS_PER_SESSION {
            return Err(Error::resource_exhausted(
                "RUM replay has too many segments",
            ));
        }
        let total_bytes = records.iter().fold(0_u64, |total, row| {
            total.saturating_add(row.bytes_uncompressed)
        });
        if total_bytes > MAX_REPLAY_SESSION_BYTES {
            return Err(Error::resource_exhausted(
                "RUM replay exceeds the read quota",
            ));
        }

        let segment_count = records.len();
        let event_capacity = records.iter().map(|row| row.event_count).sum();
        let mut events = Vec::with_capacity(event_capacity);
        for record in records {
            let path = ObjPath::parse(&record.object_key)
                .map_err(|error| Error::internal(format!("RUM replay object path: {error}")))?;
            let compressed = self
                .object_store
                .get(&path)
                .await
                .map_err(|error| Error::internal(format!("RUM replay object read: {error}")))?
                .bytes()
                .await
                .map_err(|error| Error::internal(format!("RUM replay object bytes: {error}")))?;
            let decoded =
                tokio::task::spawn_blocking(move || zstd::stream::decode_all(compressed.as_ref()))
                    .await
                    .map_err(|error| Error::internal(format!("RUM replay decode task: {error}")))?
                    .map_err(|error| Error::internal(format!("RUM replay decode: {error}")))?;
            verify_segment(&record, &decoded)?;
            for line in decoded
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
            {
                let event = serde_json::from_slice::<Value>(line)
                    .map_err(|error| Error::internal(format!("RUM replay JSON: {error}")))?;
                events.push(event);
            }
        }
        Ok((segment_count, events))
    }

    pub async fn existing_session_ids(
        &self,
        org_id: &Id,
        session_ids: &[String],
    ) -> Result<HashSet<String>> {
        self.meta.existing_session_ids(org_id, session_ids).await
    }

    pub async fn session_ids_in_window(
        &self,
        org_id: &Id,
        from_micros: i64,
        to_micros: i64,
    ) -> Result<Vec<String>> {
        let ids = self
            .meta
            .session_ids_in_window(
                org_id,
                from_micros,
                to_micros,
                MAX_REPLAY_FILTER_SESSION_IDS + 1,
            )
            .await?;
        if ids.len() > MAX_REPLAY_FILTER_SESSION_IDS {
            return Err(Error::resource_exhausted(
                "RUM replay availability filter exceeds its session limit",
            ));
        }
        Ok(ids)
    }

    /// Removes at most `limit` expired objects and their metadata rows.
    pub async fn sweep_expired(&self, cutoff_micros: i64, limit: usize) -> Result<usize> {
        let records = self.meta.list_expired(cutoff_micros, limit.max(1)).await?;
        let mut deleted_ids = Vec::with_capacity(records.len());
        for record in records {
            let path = match ObjPath::parse(&record.object_key) {
                Ok(path) => path,
                Err(error) => {
                    tracing::warn!(
                        replay_id = %record.id.0,
                        error = %error,
                        "dropping RUM replay metadata with an invalid object path"
                    );
                    deleted_ids.push(record.id.0);
                    continue;
                }
            };
            match self.object_store.delete(&path).await {
                Ok(()) | Err(object_store::Error::NotFound { .. }) => {
                    deleted_ids.push(record.id.0);
                }
                Err(error) => {
                    tracing::warn!(
                        org_id = %record.org_id.0,
                        session_id = %record.session_id,
                        error = %error,
                        "RUM replay retention object delete failed"
                    );
                }
            }
        }
        self.meta.delete_ids(&deleted_ids).await?;
        Ok(deleted_ids.len())
    }
}

fn validate_segment(session_id: &str, seq: i32, events: &[Value]) -> Result<()> {
    validate_session_id(session_id)?;
    if seq <= 0 {
        return Err(Error::invalid("RUM replay seq must be positive"));
    }
    if events.is_empty() || events.len() > MAX_REPLAY_EVENTS_PER_SEGMENT {
        return Err(Error::invalid(format!(
            "RUM replay events must contain 1..={MAX_REPLAY_EVENTS_PER_SEGMENT} items"
        )));
    }
    for event in events {
        let object = event
            .as_object()
            .ok_or_else(|| Error::invalid("RUM replay event must be an object"))?;
        match object.get("type") {
            Some(Value::Number(kind)) if kind.as_u64().is_some_and(|kind| kind <= 7) => {
                if !event_timestamp_millis(event).is_some_and(valid_timestamp_millis) {
                    return Err(Error::invalid("rrweb replay event timestamp is required"));
                }
            }
            Some(Value::String(kind)) if !kind.is_empty() && kind.len() <= 64 => {
                if !event_timestamp_millis(event).is_some_and(valid_timestamp_millis) {
                    return Err(Error::invalid("RUM timeline event timestamp is required"));
                }
            }
            _ => return Err(Error::invalid("RUM replay event type is invalid")),
        }
    }
    Ok(())
}

fn event_timestamp_millis(event: &Value) -> Option<u64> {
    event
        .get("timestamp")
        .or_else(|| event.get("ts"))
        .and_then(Value::as_u64)
}

fn valid_timestamp_millis(timestamp: u64) -> bool {
    timestamp > 0 && timestamp <= (i64::MAX as u64) / 1_000
}

fn validate_session_id(session_id: &str) -> Result<()> {
    if session_id.is_empty()
        || session_id.len() > 64
        || !session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(Error::invalid("RUM replay session_id is invalid"));
    }
    Ok(())
}

fn encode_events(events: &[Value]) -> Result<Vec<u8>> {
    let mut ndjson = Vec::new();
    for event in events {
        serde_json::to_writer(&mut ndjson, event)
            .map_err(|error| Error::internal(format!("RUM replay serialize: {error}")))?;
        ndjson.push(b'\n');
    }
    Ok(ndjson)
}

fn same_segment(existing: RumReplayRecord, content_hash: &str) -> Result<RumReplayRecord> {
    if existing.content_hash == content_hash {
        Ok(existing)
    } else {
        Err(Error::conflict(
            "RUM replay sequence already contains different content",
        ))
    }
}

fn verify_segment(record: &RumReplayRecord, decoded: &[u8]) -> Result<()> {
    let hash = hex::encode(Sha256::digest(decoded));
    if decoded.len() as u64 != record.bytes_uncompressed || hash != record.content_hash {
        return Err(Error::internal("RUM replay segment integrity check failed"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn accepts_rrweb_and_correlated_timeline_events() {
        let events = vec![
            json!({"type": 2, "timestamp": 1_727_000_000_000_u64, "data": {"node": {}}}),
            json!({"type": "click", "ts": 1_727_000_000_100_u64}),
        ];
        validate_segment("ses_valid-1", 1, &events).unwrap();
    }

    #[test]
    fn rejects_invalid_event_shape_and_session_path() {
        assert!(validate_segment("../escape", 1, &[json!({"type": 2, "timestamp": 1})]).is_err());
        assert!(validate_segment("ses_ok", 0, &[json!({"type": 2, "timestamp": 1})]).is_err());
        assert!(validate_segment("ses_ok", 1, &[json!({"type": 8, "timestamp": 1})]).is_err());
        assert!(validate_segment("ses_ok", 1, &[json!({"type": "click"})]).is_err());
    }

    #[test]
    fn encoded_segment_integrity_is_verified() {
        let events = vec![json!({"type": 2, "timestamp": 1, "data": {}})];
        let encoded = encode_events(&events).unwrap();
        let record = RumReplayRecord {
            id: Id::new(),
            org_id: Id::new(),
            application_id: "app".into(),
            session_id: "ses_integrity".into(),
            seq: 1,
            object_key: "unused".into(),
            bytes_uncompressed: encoded.len() as u64,
            event_count: 1,
            has_full_snapshot: true,
            content_hash: hex::encode(Sha256::digest(&encoded)),
            first_event_at_micros: 1_000,
            created_at: TimestampMicros(1),
        };
        verify_segment(&record, &encoded).unwrap();

        let mut corrupted = encoded;
        corrupted[0] ^= 1;
        assert!(verify_segment(&record, &corrupted).is_err());
    }
}
