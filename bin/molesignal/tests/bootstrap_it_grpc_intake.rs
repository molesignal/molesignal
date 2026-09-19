// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! gRPC intake 端到端：起 tonic server（IntakeGrpc → app::IntakeService → IntakeWorker），
//! tonic client push 100 条 → 验 buffer 收到 → 显式 flush_one → 验 FileCatalog 原子发布。
//!
//! 无 docker：用 in-memory StreamRepository / FileCatalog。

#![allow(clippy::field_reassign_with_default)]

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};

use async_trait::async_trait;
use molesignal::{
    api::grpc::intake_server::IntakeGrpc,
    app::intake::IntakeService as AppIntakeService,
    bootstrap::roles::intake::IntakeWorker,
    config::{IntakeSettings, ObjectStoreSettings},
    domain::{
        intake::{IntakeSink, RawEvent},
        storage::{
            CatalogSnapshot, CommitFlush, DataSegment, DatasetSelection, DatasetState, FileCatalog,
            FlushCommitResult, OrganizationScope, PhysicalDataset, PhysicalDatasetId,
            PhysicalDatasetSpec, ReplaceSegments, TombstoneSegments, UpdateArtifact,
            WalCheckpointView, builtin_registry, primary_dataset_type,
        },
        stream::{
            FieldDef, FieldType, Retention, Schema, StreamDefinition, StreamRepository, StreamType,
        },
    },
    infra::{
        intake::{BufferPool, DatasetResolver, WalPool},
        segment_wal::FsyncPolicy,
        storage::{object, parquet::writer::ParquetWriter},
    },
    protocol::intake::v1::{
        PushRequest, StreamType as ProtoStreamType, intake_service_client::IntakeServiceClient,
    },
    shared::{Error, Result, health::Probe, ids::Id, time::TimestampMicros},
};
use serde_json::json;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;

// =====================================================================
//  In-memory FileCatalog
// =====================================================================
#[derive(Default)]
struct InMemFileCatalog {
    datasets: StdMutex<HashMap<(String, String, String), PhysicalDataset>>,
    committed: StdMutex<Vec<DataSegment>>,
}

#[async_trait]
impl FileCatalog for InMemFileCatalog {
    async fn ensure_datasets(
        &self,
        scope: &OrganizationScope,
        logical_stream_id: &Id,
        specs: &[PhysicalDatasetSpec],
    ) -> Result<Vec<PhysicalDataset>> {
        let now = TimestampMicros::now().0;
        let mut map = self.datasets.lock().unwrap();
        Ok(specs
            .iter()
            .map(|spec| {
                map.entry((
                    scope.organization_id.as_str().to_owned(),
                    logical_stream_id.as_str().to_owned(),
                    spec.dataset_type.as_str().to_owned(),
                ))
                .or_insert_with(|| PhysicalDataset {
                    id: PhysicalDatasetId::generate(),
                    organization_id: scope.organization_id.clone(),
                    logical_stream_id: logical_stream_id.clone(),
                    dataset_type: spec.dataset_type.clone(),
                    dataset_type_version: spec.dataset_type_version,
                    partition_policy: spec.partition_policy.clone(),
                    storage_policy: spec.storage_policy.clone(),
                    index_policy: spec.index_policy.clone(),
                    catalog_version: 0,
                    state: DatasetState::Active,
                    created_at_micros: now,
                    updated_at_micros: now,
                })
                .clone()
            })
            .collect())
    }

    async fn list_datasets(
        &self,
        scope: &OrganizationScope,
        logical_stream_id: &Id,
    ) -> Result<Vec<PhysicalDataset>> {
        Ok(self
            .datasets
            .lock()
            .unwrap()
            .iter()
            .filter(|(key, _)| {
                key.0 == scope.organization_id.as_str() && key.1 == logical_stream_id.as_str()
            })
            .map(|(_, dataset)| dataset.clone())
            .collect())
    }

    async fn snapshot(
        &self,
        _scope: &OrganizationScope,
        _selection: DatasetSelection,
    ) -> Result<CatalogSnapshot> {
        Err(Error::internal("unsupported in test"))
    }

    async fn commit_flush(
        &self,
        _scope: &OrganizationScope,
        command: CommitFlush,
    ) -> Result<FlushCommitResult> {
        self.committed.lock().unwrap().extend(command.segments);
        Ok(FlushCommitResult {
            catalog_version: 1,
            already_committed: false,
        })
    }

    async fn replace_segments(
        &self,
        _scope: &OrganizationScope,
        _command: ReplaceSegments,
    ) -> Result<u64> {
        Err(Error::internal("unsupported in test"))
    }

    async fn update_artifact(
        &self,
        _scope: &OrganizationScope,
        _command: UpdateArtifact,
    ) -> Result<()> {
        Err(Error::internal("unsupported in test"))
    }

    async fn tombstone_segments(
        &self,
        _scope: &OrganizationScope,
        _command: TombstoneSegments,
    ) -> Result<u64> {
        Err(Error::internal("unsupported in test"))
    }

    async fn wal_checkpoints(
        &self,
        _scope: &OrganizationScope,
        _dataset_id: &PhysicalDatasetId,
    ) -> Result<Vec<WalCheckpointView>> {
        Ok(Vec::new())
    }
}

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

fn sample_stream() -> StreamDefinition {
    StreamDefinition {
        id: Id::new(),
        org_id: Id::from_string("orga"),
        name: "app".into(),
        stream_type: StreamType::LOGS,
        schema: Schema {
            fields: vec![FieldDef {
                name: "level".into(),
                data_type: FieldType::Utf8,
                nullable: false,
                index_type: None,
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
        "node-test",
        64 * 1024,
        FsyncPolicy::none_default(),
    ));
    let buffer = Arc::new(BufferPool::new());
    let streams = InMemStreams::with(stream.clone());
    let stream_repo: Arc<dyn StreamRepository> = streams.clone();
    let parquet_writer = Arc::new(ParquetWriter::new(store));
    let probe = Arc::new(Probe::new());

    let catalog = Arc::new(InMemFileCatalog::default());
    let resolver = Arc::new(DatasetResolver::new(
        catalog.clone(),
        Arc::new(builtin_registry()),
    ));
    let worker = Arc::new(IntakeWorker::new(
        wal_pool.clone(),
        buffer.clone(),
        stream_repo.clone(),
        resolver.clone(),
        catalog.clone(),
        parquet_writer,
        probe,
        IntakeSettings::default(),
    ));
    // 直接 replay（无残留 WAL，期望立即 ready）
    worker.recover_and_replay().await.unwrap();

    let intake_service = Arc::new(AppIntakeService::new(
        worker.clone() as Arc<dyn IntakeSink>,
        stream_repo,
    ));
    let grpc = IntakeGrpc::new(intake_service).into_server();

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
        match IntakeServiceClient::connect(format!("http://{local_addr}")).await {
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
    let key = resolver
        .resolve(&stream, primary_dataset_type(stream.stream_type).unwrap())
        .await
        .unwrap()
        .dataset
        .id
        .clone();
    {
        let buf = buffer.get(&key).expect("buffer exists");
        let guard = buf.records().lock().await;
        assert_eq!(guard.row_count(), 100);
    }

    // 显式 flush
    worker.flush_one(&key).await.expect("flush_one");
    let segments = catalog.committed.lock().unwrap();
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].row_count, 100);

    server.abort();
}
