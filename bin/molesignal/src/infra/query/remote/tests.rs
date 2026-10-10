// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use parking_lot::Mutex;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;

use super::*;
use crate::{
    api::grpc::query_server::QueryExecutionGrpc,
    domain::{query::QueryLanguage, stream::StreamType},
    shared::{
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

/// 记录收到的请求，返回带标记的结果；`fail` 让所有调用以 400 失败。
#[derive(Default)]
struct Fake {
    seen: Mutex<Vec<String>>,
    fail: bool,
}

impl Fake {
    fn answer(&self, tag: &str, req: &QueryRequest) -> Result<QueryResult> {
        self.seen.lock().push(format!("{tag}:{}", req.statement));

        if self.fail {
            return Err(Error::invalid("query could not be planned"));
        }

        Ok(QueryResult {
            columns: vec![tag.to_owned()],
            rows: vec![vec![serde_json::json!(req.statement)]],
            scanned_rows: 7,
            took_ms: 1,
            federation: None,
        })
    }
}

#[async_trait]
impl QueryEngine for Fake {
    async fn execute(&self, req: QueryRequest) -> Result<QueryResult> {
        self.answer("sql", &req)
    }

    async fn execute_dataset(
        &self,
        req: QueryRequest,
        dataset_type: DatasetTypeId,
    ) -> Result<QueryResult> {
        self.answer(&format!("dataset={dataset_type}"), &req)
    }

    async fn explain(&self, req: QueryRequest) -> Result<String> {
        self.seen.lock().push(format!("explain:{}", req.statement));
        Ok("Projection: count(*)".into())
    }
}

#[async_trait]
impl PromqlEngine for Fake {
    async fn execute(&self, req: QueryRequest) -> Result<QueryResult> {
        self.answer("promql", &req)
    }

    async fn query_exemplars(&self, req: QueryRequest) -> Result<PrometheusExemplarQueryResult> {
        self.seen
            .lock()
            .push(format!("exemplars:{}", req.statement));

        Ok(PrometheusExemplarQueryResult {
            series: Vec::new(),
            truncated: true,
        })
    }
}

async fn serve(engine: Arc<Fake>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let service = QueryExecutionGrpc::new(engine.clone(), engine).into_server();

    tokio::spawn(
        Server::builder()
            .add_service(service)
            .serve_with_incoming(TcpListenerStream::new(listener)),
    );

    addr
}

struct Peers(Vec<PeerInfo>);

#[async_trait]
impl ClusterRegistry for Peers {
    async fn list_role(&self, role: PeerRole) -> Vec<PeerInfo> {
        if role == PeerRole::Querier {
            self.0.clone()
        } else {
            Vec::new()
        }
    }
}

fn peer(node_id: &str, addr: &str) -> PeerInfo {
    PeerInfo {
        node_id: node_id.into(),
        advertise_addr: addr.into(),
        roles: vec![PeerRole::Querier],
    }
}

fn client(peers: Vec<PeerInfo>) -> Arc<RemoteQueryClient> {
    RemoteQueryClient::new(Arc::new(Peers(peers)), 8 * 1024 * 1024)
}

fn request(statement: &str, language: QueryLanguage) -> QueryRequest {
    QueryRequest {
        org_id: Id::from_string("org-1"),
        language,
        statement: statement.into(),
        time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(10_000_000)),
        stream: Some(crate::domain::query::StreamHint {
            name: "app".into(),
            stream_type: StreamType::LOGS,
        }),
        limit: Some(5),
        federation_clusters: Vec::new(),
    }
}

#[tokio::test]
async fn sql_queries_run_on_the_querier_and_return_its_result() {
    let engine = Arc::new(Fake::default());
    let addr = serve(engine.clone()).await;
    let sql = RemoteSqlEngine::new(client(vec![peer("q1", &addr)]));

    let result = sql
        .execute(request("SELECT count(*) FROM app", QueryLanguage::Sql))
        .await
        .unwrap();

    assert_eq!(result.columns, vec!["sql"]);
    assert_eq!(result.rows[0][0], "SELECT count(*) FROM app");
    assert_eq!(result.scanned_rows, 7);
    assert_eq!(*engine.seen.lock(), vec!["sql:SELECT count(*) FROM app"]);
}

#[tokio::test]
async fn dataset_queries_name_the_physical_dataset() {
    let engine = Arc::new(Fake::default());
    let addr = serve(engine.clone()).await;
    let sql = RemoteSqlEngine::new(client(vec![peer("q1", &addr)]));
    let dataset = DatasetTypeId::new("builtin.logs.records").unwrap();

    sql.execute_dataset(request("SELECT 1 FROM app", QueryLanguage::Sql), dataset)
        .await
        .unwrap();

    assert_eq!(
        *engine.seen.lock(),
        vec!["dataset=builtin.logs.records:SELECT 1 FROM app"]
    );
}

#[tokio::test]
async fn explain_and_promql_operations_are_forwarded() {
    let engine = Arc::new(Fake::default());
    let addr = serve(engine.clone()).await;
    let shared = client(vec![peer("q1", &addr)]);
    let sql = RemoteSqlEngine::new(shared.clone());
    let promql = RemotePromqlEngine::new(shared);

    let plan = sql
        .explain(request("SELECT count(*) FROM app", QueryLanguage::Sql))
        .await
        .unwrap();
    let samples = promql
        .execute(request("up", QueryLanguage::Promql))
        .await
        .unwrap();
    let exemplars = promql
        .query_exemplars(request("up", QueryLanguage::Promql))
        .await
        .unwrap();

    assert_eq!(plan, "Projection: count(*)");
    assert_eq!(samples.columns, vec!["promql"]);
    assert!(exemplars.truncated);
    assert_eq!(
        *engine.seen.lock(),
        vec![
            "explain:SELECT count(*) FROM app",
            "promql:up",
            "exemplars:up"
        ]
    );
}

#[tokio::test]
async fn engine_errors_keep_their_kind() {
    let engine = Arc::new(Fake {
        fail: true,
        ..Fake::default()
    });
    let addr = serve(engine).await;
    let sql = RemoteSqlEngine::new(client(vec![peer("q1", &addr)]));

    let error = sql
        .execute(request("SELECT nope FROM app", QueryLanguage::Sql))
        .await
        .unwrap_err();

    assert_eq!(error.http_status_code(), 400);
    assert!(error.to_string().contains("could not be planned"));
}

#[tokio::test]
async fn an_unreachable_querier_is_skipped() {
    let engine = Arc::new(Fake::default());
    let live = serve(engine.clone()).await;
    let sql = RemoteSqlEngine::new(client(vec![peer("down", "127.0.0.1:1"), peer("up", &live)]));

    sql.execute(request("SELECT 1 FROM app", QueryLanguage::Sql))
        .await
        .unwrap();

    assert_eq!(engine.seen.lock().len(), 1);
}

#[tokio::test]
async fn without_a_querier_the_query_is_unavailable() {
    let sql = RemoteSqlEngine::new(client(Vec::new()));
    let error = sql
        .execute(request("SELECT 1 FROM app", QueryLanguage::Sql))
        .await
        .unwrap_err();
    assert_eq!(error.http_status_code(), 503);
}
