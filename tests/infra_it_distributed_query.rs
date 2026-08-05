// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 分布式查询端到端（单进程 self-loopback 模拟）：
//!
//! - 起一个 Flight server 在 localhost:0（监听本地随机端口）
//! - mock ClusterRegistry 返回 2 个 peer，advertise_addr 都指向上面的 server
//! - 写 6 个 parquet → DistributedDataFusionEngine.execute SELECT count(*)
//! - 验：scanned_rows = 6 个 parquet 全部行数；count 正确

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};

use arrow::{
    array::{Int64Array, RecordBatch, TimestampMicrosecondArray},
    datatypes::{DataType, Field, Schema as ArrowSchema, TimeUnit},
};
use async_trait::async_trait;
use molesignal::{
    app::cluster::{ClusterRegistry, PeerInfo, PeerRole},
    domain::{
        query::{QueryEngine, QueryLanguage, QueryRequest, StreamHint},
        storage::{ParquetFileMeta, ParquetFileMetaRepository},
        stream::{FieldDef, FieldType, Retention, Schema, StreamDefinition, StreamType},
    },
    infra::{
        query::distributed::DistributedDataFusionEngine,
        search::datafusion_engine::DataFusionEngine, storage::parquet::writer::ParquetWriter,
    },
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};
use object_store::{ObjectStore, local::LocalFileSystem};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;

// ---- in-mem ParquetFileMetaRepository ----
#[derive(Default)]
struct InMemParquetFileMeta {
    inner: StdMutex<HashMap<String, ParquetFileMeta>>,
}
#[async_trait]
impl ParquetFileMetaRepository for InMemParquetFileMeta {
    async fn insert(&self, file: ParquetFileMeta) -> Result<()> {
        self.inner.lock().unwrap().insert(file.id.0.clone(), file);
        Ok(())
    }
    async fn find(
        &self,
        org: &Id,
        stream: &str,
        st: StreamType,
        range: TimeRange,
    ) -> Result<Vec<ParquetFileMeta>> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .values()
            .filter(|f| {
                !f.deleted
                    && &f.org_id == org
                    && f.stream == stream
                    && f.stream_type == st
                    && f.time_range.end.0 >= range.start.0
                    && f.time_range.start.0 <= range.end.0
            })
            .cloned()
            .collect())
    }
    async fn replace(&self, _: &[Id], _: Vec<ParquetFileMeta>) -> Result<()> {
        Err(Error::internal("not supported"))
    }
    async fn mark_deleted(&self, _: &[Id]) -> Result<usize> {
        Err(Error::internal("not supported"))
    }
}

// ---- mock registry：list_role 返 2 个 peer 同 addr ----
struct TwoPeerRegistry {
    addr: String,
}
#[async_trait]
impl ClusterRegistry for TwoPeerRegistry {
    async fn list_role(&self, _role: PeerRole) -> Vec<PeerInfo> {
        vec![
            PeerInfo {
                node_id: "peer-a".into(),
                advertise_addr: self.addr.clone(),
                roles: vec![PeerRole::Querier],
            },
            PeerInfo {
                node_id: "peer-b".into(),
                advertise_addr: self.addr.clone(),
                roles: vec![PeerRole::Querier],
            },
        ]
    }
}

fn stream_named(name: &str) -> StreamDefinition {
    StreamDefinition {
        id: Id::new(),
        org_id: Id::from_string("orga"),
        name: name.into(),
        stream_type: StreamType::Logs,
        schema: Schema {
            fields: vec![FieldDef {
                name: "n".into(),
                data_type: FieldType::Int64,
                nullable: false,
                indexed: false,
                encrypted: false,
                exact: false,
            }],
        },
        retention: Some(Retention { days: 7 }),
        created_at: TimestampMicros::now(),
        updated_at: TimestampMicros::now(),
    }
}

fn batch(ts_us: i64, vals: &[i64]) -> RecordBatch {
    let schema = Arc::new(ArrowSchema::new(vec![
        Field::new(
            "_timestamp",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        ),
        Field::new("n", DataType::Int64, false),
    ]));
    let ts = TimestampMicrosecondArray::from(
        (0..vals.len() as i64)
            .map(|i| ts_us + i)
            .collect::<Vec<_>>(),
    )
    .with_timezone("UTC");
    let n = Int64Array::from(vals.to_vec());
    RecordBatch::try_new(schema, vec![Arc::new(ts), Arc::new(n)]).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn distributed_count_star_via_flight_loopback() {
    let (scanned, n) = distributed_count_star("logs", "logs").await;
    // 6 个 parquet × 10 行 = 60 总行 — 通过 2 个 peer 各扫一半（≈ 30 行）然后 coordinator union
    assert_eq!(scanned, 60, "all 60 rows came back through Flight");
    assert_eq!(n, 60);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn distributed_query_handles_stream_name_needing_quotes() {
    // 生产 ingest 会拒绝空格，但查询引擎仍应正确处理内部/测试仓库提供的 quoted name。
    // coordinator 发给 peer 的 scan SQL 若不给流名加引号，`SELECT * FROM my stream`
    // 会被解析成 `my AS stream` 而找不到表。
    let (scanned, n) = distributed_count_star("my stream", "\"my stream\"").await;
    assert_eq!(scanned, 60);
    assert_eq!(n, 60);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn distributed_query_handles_dotted_stream_name() {
    // OTLP 的 service 名带点极常见（checkout.api）。register_table 若把流名交给
    // Into<TableReference> 推断，`app.logs` 会被拆成 schema=app / table=logs，而 SQL 里的
    // `"app.logs"` 是单一标识符，两者对不上 —— 加引号也救不了，必须用 TableReference::bare。
    let (scanned, n) = distributed_count_star("app.logs", "\"app.logs\"").await;
    assert_eq!(scanned, 60);
    assert_eq!(n, 60);
}

/// 建 6 个 parquet（每个 10 行）→ 起 in-proc Flight server → 过 2-peer 分布式引擎跑
/// `SELECT count(*) FROM {from_clause}`。返回 `(scanned_rows, count)`。
async fn distributed_count_star(stream_name: &str, from_clause: &str) -> (u64, i64) {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let writer = ParquetWriter::new(store.clone());
    let repo: Arc<InMemParquetFileMeta> = Arc::new(InMemParquetFileMeta::default());
    let stream = stream_named(stream_name);

    // 6 个 parquet，每个 10 行
    for i in 0..6 {
        let b = batch(1_000_000 + i * 10_000, &(0..10).collect::<Vec<i64>>());
        let m = writer.flush(&stream, b).await.unwrap();
        repo.insert(m).await.unwrap();
    }

    // 起 in-proc Flight server
    let flight = molesignal::api::grpc::flight::FlightGrpc::new(store.clone()).into_server();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local_addr: SocketAddr = listener.local_addr().unwrap();
    let incoming = TcpListenerStream::new(listener);
    let server = tokio::spawn(async move {
        Server::builder()
            .add_service(flight)
            .serve_with_incoming(incoming)
            .await
            .unwrap();
    });
    // 等 server 起来
    for _ in 0..50 {
        if tokio::net::TcpStream::connect(local_addr).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let registry: Arc<dyn ClusterRegistry> = Arc::new(TwoPeerRegistry {
        addr: local_addr.to_string(),
    });
    let local = Arc::new(DataFusionEngine::new(
        repo.clone() as Arc<dyn ParquetFileMetaRepository>,
        store.clone(),
    ));
    let dist = DistributedDataFusionEngine::new(
        local,
        registry,
        repo.clone() as Arc<dyn ParquetFileMetaRepository>,
        store.clone(),
    );

    let req = QueryRequest {
        org_id: stream.org_id.clone(),
        language: QueryLanguage::Sql,
        statement: format!("SELECT count(*) AS n FROM {from_clause}"),
        time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX)),
        stream: Some(StreamHint {
            name: stream_name.into(),
            stream_type: StreamType::Logs,
        }),
        limit: None,
        federation_clusters: Vec::new(),
    };
    let res = dist.execute(req).await.expect("distributed query");
    assert_eq!(res.rows.len(), 1);
    let n_val = &res.rows[0][0];
    let n = n_val
        .as_i64()
        .or_else(|| n_val.as_u64().map(|u| u as i64))
        .unwrap();

    server.abort();
    (res.scanned_rows, n)
}
