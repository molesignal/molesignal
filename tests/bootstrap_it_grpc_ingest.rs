// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! gRPC ingest 端到端：起 tonic server（IngestGrpc → app::IngestService → IngesterWorker），
//! tonic client push 100 条 → 验 buffer 收到 → 显式 flush_one → 验 ParquetFileMeta 落库。
//!
//! 无 docker：用 in-mem StreamRepository / ParquetFileMetaRepository（与 it_ingester_flush 不同，
//! 那个是真 postgres）。

#![allow(clippy::field_reassign_with_default)]

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};

use async_trait::async_trait;
use molesignal::{
    api::grpc::ingest_server::IngestGrpc,
    app::ingestion::IngestService as AppIngestService,
    bootstrap::roles::ingester::IngesterWorker,
    config::{CacheLayerSettings, IngesterSettings, ObjectStoreSettings},
    domain::{
        ingestion::{IngestSink, RawEvent},
        storage::{ParquetFileMeta, ParquetFileMetaRepository, PhysicalDatasetKind},
        stream::{
            FieldDef, FieldType, Retention, Schema, StreamDefinition, StreamRepository, StreamType,
        },
    },
    infra::{
        caching::ParquetFileMetaCache,
        ingester::{BufferPool, WalPool},
        segment_wal::{FsyncPolicy, StaticTermSource, TermSource},
        storage::{object, parquet::writer::ParquetWriter},
    },
    protocol::ingest::v1::{
        PushRequest, StreamType as ProtoStreamType, ingest_service_client::IngestServiceClient,
    },
    shared::{
        Error, Result,
        health::Probe,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};
use serde_json::json;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;

// =====================================================================
//  In-mem repos
// =====================================================================
struct InMemStreams {
    inner: StdMutex<HashMap<(Id, String, StreamType), StreamDefinition>>,
}
impl InMemStreams {
    fn with(def: StreamDefinition) -> Arc<Self> {
        let mut m = HashMap::new();
        m.insert((def.org_id.clone(), def.name.clone(), def.stream_type), def);
        Arc::new(Self {
            inner: StdMutex::new(m),
        })
    }
}
#[async_trait]
impl StreamRepository for InMemStreams {
    async fn create(&self, def: StreamDefinition) -> Result<StreamDefinition> {
        self.inner.lock().unwrap().insert(
            (def.org_id.clone(), def.name.clone(), def.stream_type),
            def.clone(),
        );
        Ok(def)
    }
    async fn update_schema(&self, _id: &Id, _schema: Schema) -> Result<()> {
        Ok(())
    }
    async fn get(&self, org_id: &Id, name: &str, st: StreamType) -> Result<StreamDefinition> {
        self.inner
            .lock()
            .unwrap()
            .get(&(org_id.clone(), name.to_string(), st))
            .cloned()
            .ok_or_else(|| Error::not_found(format!("stream {name}")))
    }
    async fn list(&self, _org_id: &Id) -> Result<Vec<StreamDefinition>> {
        Ok(self.inner.lock().unwrap().values().cloned().collect())
    }
    async fn delete(&self, _id: &Id) -> Result<()> {
        Ok(())
    }
}

#[derive(Default)]
struct InMemParquetFileMeta {
    inner: StdMutex<Vec<ParquetFileMeta>>,
}
#[async_trait]
impl ParquetFileMetaRepository for InMemParquetFileMeta {
    async fn insert(&self, file: ParquetFileMeta) -> Result<()> {
        self.inner.lock().unwrap().push(file);
        Ok(())
    }
    async fn find(
        &self,
        org_id: &Id,
        stream: &str,
        st: StreamType,
        _range: TimeRange,
    ) -> Result<Vec<ParquetFileMeta>> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .iter()
            .filter(|f| &f.org_id == org_id && f.stream == stream && f.stream_type == st)
            .cloned()
            .collect())
    }
    async fn replace(&self, _merged: &[Id], _new: Vec<ParquetFileMeta>) -> Result<()> {
        Ok(())
    }

    async fn mark_deleted(&self, _ids: &[Id]) -> Result<usize> {
        Ok(0)
    }
}

fn sample_stream() -> StreamDefinition {
    StreamDefinition {
        id: Id::new(),
        org_id: Id::from_string("orga"),
        name: "app".into(),
        stream_type: StreamType::Logs,
        schema: Schema {
            fields: vec![FieldDef {
                name: "level".into(),
                data_type: FieldType::Utf8,
                nullable: false,
                indexed: true,
                encrypted: false,
                exact: false,
            }],
        },
        retention: Some(Retention { days: 7 }),
        created_at: TimestampMicros::now(),
        updated_at: TimestampMicros::now(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grpc_push_100_events_lands_in_buffer_and_flush() {
    let stream = sample_stream();
    let object_root = tempfile::tempdir().unwrap();
    let wal_root = tempfile::tempdir().unwrap();
    let object_cfg = ObjectStoreSettings {
        backend: "local".into(),
        root: object_root.path().to_string_lossy().into(),
        ..Default::default()
    };
    let store = object::build(&object_cfg).unwrap();
    let wal_pool = Arc::new(WalPool::new(
        wal_root.path(),
        64 * 1024,
        FsyncPolicy::none_default(),
        Arc::new(StaticTermSource(1)) as Arc<dyn TermSource>,
    ));
    let buffer = Arc::new(BufferPool::new());
    let streams = InMemStreams::with(stream.clone());
    let stream_repo: Arc<dyn StreamRepository> = streams.clone();
    let parquet_file_meta = Arc::new(InMemParquetFileMeta::default());
    let parquet_file_meta_repo: Arc<dyn ParquetFileMetaRepository> = parquet_file_meta.clone();
    let parquet_writer = Arc::new(ParquetWriter::new(store));
    let cache = Arc::new(ParquetFileMetaCache::new(CacheLayerSettings::new(100, 60)));
    let probe = Arc::new(Probe::new());

    let worker = Arc::new(IngesterWorker::new(
        wal_pool.clone(),
        buffer.clone(),
        stream_repo.clone(),
        parquet_file_meta_repo,
        parquet_writer,
        Some(cache),
        probe,
        IngesterSettings::default(),
    ));
    // 直接 replay（无残留 WAL，期望立即 ready）
    worker.recover_and_replay().await.unwrap();

    let ingest_service = Arc::new(AppIngestService::new(
        worker.clone() as Arc<dyn IngestSink>,
        stream_repo,
    ));
    let grpc = IngestGrpc::new(ingest_service).into_server();

    // 起 tonic server 在随机端口
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local_addr: SocketAddr = listener.local_addr().unwrap();
    let incoming = TcpListenerStream::new(listener);
    let server = tokio::spawn(async move {
        Server::builder()
            .add_service(grpc)
            .serve_with_incoming(incoming)
            .await
            .unwrap();
    });

    // 等 server 真起来（几次重试）
    let mut client_opt = None;
    for _ in 0..50 {
        match IngestServiceClient::connect(format!("http://{local_addr}")).await {
            Ok(c) => {
                client_opt = Some(c);
                break;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
        }
    }
    let mut client = client_opt.expect("connect to grpc server");

    // 推 100 条
    let events: Vec<RawEvent> = (0..100)
        .map(|i| {
            let mut f = serde_json::Map::new();
            f.insert("level".into(), json!("info"));
            RawEvent {
                timestamp: TimestampMicros(1_700_000_000_000_000 + i as i64 * 1000),
                fields: f,
            }
        })
        .collect();
    let payload = serde_json::to_vec(&events).unwrap();
    let resp = client
        .push(PushRequest {
            batch_id: String::new(),
            org_id: stream.org_id.0.clone(),
            stream: stream.name.clone(),
            stream_type: ProtoStreamType::Logs as i32,
            payload: payload.into(),
            received_at_micros: 0,
        })
        .await
        .expect("push");
    let resp = resp.into_inner();
    assert_eq!(resp.accepted, 100, "all events accepted");
    assert_eq!(resp.rejected, 0);

    // 验 buffer 收到 100 行
    let key = (
        stream.org_id.clone(),
        stream.stream_type,
        stream.name.clone(),
        PhysicalDatasetKind::Raw,
    );
    {
        let buf = buffer.get(&key).expect("buffer exists");
        let guard = buf.lock().await;
        assert_eq!(guard.row_count(), 100);
    }

    // 显式 flush
    worker.flush_one(&key).await.expect("flush_one");
    let files = parquet_file_meta.inner.lock().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].rows, 100);

    server.abort();
}
