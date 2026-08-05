// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Continuous profile 的 object archive + metadata stream 统一存储服务。

use std::sync::Arc;

use object_store::ObjectStore;
use serde_json::Value;

use crate::{
    app::ingestion::IngestService,
    domain::{
        ingestion::IngestBatch,
        stream::{DEFAULT_PROFILE_STREAM, StreamType},
    },
    infra::profiles::{self, NormalizedProfile},
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct ProfileStorageService {
    object_store: Arc<dyn ObjectStore>,
    ingestion: Arc<IngestService>,
}

impl ProfileStorageService {
    pub fn new(object_store: Arc<dyn ObjectStore>, ingestion: Arc<IngestService>) -> Self {
        Self {
            object_store,
            ingestion,
        }
    }

    /// 公共 upload/Pyroscope/OTLP 入口只能写 `profiles/default`。
    pub async fn store_public(
        &self,
        org_id: &Id,
        normalized: &NormalizedProfile,
        raw_pprof: &[u8],
    ) -> Result<()> {
        let event = self
            .archive_metadata_event(org_id, normalized, raw_pprof)
            .await?;
        let batch = IngestBatch {
            batch_id: Id::new(),
            org_id: org_id.clone(),
            stream: DEFAULT_PROFILE_STREAM.into(),
            stream_type: StreamType::Profiles,
            events: vec![event],
            received_at: TimestampMicros::now(),
        };
        self.ingestion.ingest(batch).await?;
        Ok(())
    }

    /// 归档 profile blob 并构造 metadata 行，但不选择 ingestion origin。split-role
    /// self telemetry 用它归档后交给 role-aware delivery。
    pub(crate) async fn archive_metadata_event(
        &self,
        org_id: &Id,
        normalized: &NormalizedProfile,
        raw_pprof: &[u8],
    ) -> Result<crate::domain::ingestion::RawEvent> {
        let timestamp = if normalized.start_time_micros > 0 {
            TimestampMicros(normalized.start_time_micros)
        } else {
            TimestampMicros::now()
        };
        let profile_id = Id::new();
        let object_key = profiles::archive_object_key(
            org_id,
            &normalized.service,
            normalized.profile_type.as_str(),
            timestamp.0,
            &profile_id,
        );
        let archived = profiles::put_archive(&self.object_store, &object_key, raw_pprof).await?;
        let mut event = profiles::metadata_event(normalized, &object_key, archived, timestamp);
        event
            .fields
            .insert("id".into(), Value::from(profile_id.to_string()));
        Ok(event)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn public_and_internal_profile_streams_are_distinct() {
        assert_eq!(super::DEFAULT_PROFILE_STREAM, "default");
        assert_eq!(
            crate::domain::stream::MOLESIGNAL_SYSTEM_STREAM,
            "_molesignal"
        );
    }
}
