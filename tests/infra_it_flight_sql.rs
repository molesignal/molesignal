// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Flight SQL 端到端（spec flight-sql）：真 tonic server + `FlightSqlServiceClient`。
//!
//! - 正向：Handshake（basic auth，password = `ms_` token）→ get_tables →
//!   `SELECT ... FROM logs.app` → 校验行数 / Arrow 类型。
//! - 反向：无效 token、跨 org ticket 重放、DML 拒绝。
//! - registry：慢查询执行中出现在 `QueryRegistry::list_for`，cancel 后客户端收错。

use std::{sync::Arc, time::Duration};

use arrow::array::{Array, Int64Array, RecordBatch, StringArray, TimestampMicrosecondArray};
use arrow_flight::{
    Ticket,
    sql::{
        CommandGetTables, ProstMessageExt, TicketStatementQuery, client::FlightSqlServiceClient,
    },
};
use async_trait::async_trait;
use futures::TryStreamExt;
use molesignal::{
    api::grpc::flight::sql::server::FlightSqlGrpc,
    app::{
        iam::{IamContext, IamContextEnricher, IamService, hash_password},
        query::QueryService,
    },
    config::{AuthSettings, FlightSqlSettings},
    domain::{
        iam::{
            IamAssignedRole, IamMembership, IamMembershipRepository, Organization,
            OrganizationRepository, User, UserRepository,
            api_token::{ApiToken, ApiTokenRepository, ManagedApiToken},
        },
        query::{PromqlEngine, QueryEngine, QueryRequest, QueryResult},
        storage::{ParquetFileMeta, ParquetFileMetaRepository},
        stream::{
            FieldDef, FieldType, Retention, Schema, StreamDefinition, StreamRepository, StreamType,
        },
    },
    infra::{
        persistence::repositories::api_tokens::{
            assemble_token, generate_token_parts, hash_secret,
        },
        search::datafusion_engine::DataFusionEngine,
        storage::{arrow_schema::to_arrow, parquet::writer::ParquetWriter},
    },
    protocol::query::v1::FlightSqlTicket,
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};
use object_store::{ObjectStore, local::LocalFileSystem};
use parking_lot::Mutex;
use prost::Message;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Channel;

// === in-memory fixtures ===

struct MemParquetFileMetaRepo {
    files: Mutex<Vec<ParquetFileMeta>>,
}

struct TestIamContextEnricher;

#[async_trait]
impl IamContextEnricher for TestIamContextEnricher {
    async fn enrich_iam_context(&self, context: &mut IamContext) -> Result<()> {
        context.permissions.insert("streams.read".into());
        context.permissions.insert("streams.query".into());
        context.policy_version = 1;
        Ok(())
    }
}

#[async_trait]
impl ParquetFileMetaRepository for MemParquetFileMetaRepo {
    async fn insert(&self, file: ParquetFileMeta) -> Result<()> {
        self.files.lock().push(file);
        Ok(())
    }
    async fn find(
        &self,
        org_id: &Id,
        stream: &str,
        stream_type: StreamType,
        time_range: TimeRange,
    ) -> Result<Vec<ParquetFileMeta>> {
        Ok(self
            .files
            .lock()
            .iter()
            .filter(|f| {
                &f.org_id == org_id
                    && f.stream == stream
                    && f.stream_type == stream_type
                    && !f.deleted
                    && f.time_range.end.0 >= time_range.start.0
                    && f.time_range.start.0 <= time_range.end.0
            })
            .cloned()
            .collect())
    }
    async fn replace(&self, _merged_ids: &[Id], _new_files: Vec<ParquetFileMeta>) -> Result<()> {
        unimplemented!()
    }
    async fn mark_deleted(&self, _ids: &[Id]) -> Result<usize> {
        unimplemented!()
    }
}

struct MemApiTokenRepo {
    tokens: Mutex<Vec<ApiToken>>,
}

#[async_trait]
impl ApiTokenRepository for MemApiTokenRepo {
    async fn create(&self, t: ApiToken) -> Result<ApiToken> {
        self.tokens.lock().push(t.clone());
        Ok(t)
    }
    async fn find_by_prefix(&self, prefix: &str) -> Result<Option<ApiToken>> {
        Ok(self
            .tokens
            .lock()
            .iter()
            .find(|t| t.prefix == prefix)
            .cloned())
    }
    async fn list_by_org(&self, _org_id: &Id) -> Result<Vec<ApiToken>> {
        unimplemented!()
    }
    async fn get(&self, _org_id: &Id, _id: &Id) -> Result<ApiToken> {
        unimplemented!()
    }
    async fn mark_revoked(&self, _org_id: &Id, _id: &Id) -> Result<()> {
        unimplemented!()
    }
    async fn touch_last_used(&self, _prefix: &str, _at: TimestampMicros) -> Result<()> {
        Ok(())
    }
    async fn ensure_default(
        &self,
        _org_id: &Id,
        _user_id: &Id,
        _role_id: &Id,
    ) -> Result<ManagedApiToken> {
        unimplemented!()
    }
    async fn ensure_rum_client(
        &self,
        _org_id: &Id,
        _user_id: &Id,
        _role_id: &Id,
        _application_id: &str,
    ) -> Result<ManagedApiToken> {
        unimplemented!()
    }
}

struct MemStreamRepo {
    defs: Mutex<Vec<StreamDefinition>>,
}

#[async_trait]
impl StreamRepository for MemStreamRepo {
    async fn create(&self, def: StreamDefinition) -> Result<StreamDefinition> {
        self.defs.lock().push(def.clone());
        Ok(def)
    }
    async fn update_schema(&self, _id: &Id, _schema: Schema) -> Result<()> {
        unimplemented!()
    }
    async fn get(
        &self,
        org_id: &Id,
        name: &str,
        stream_type: StreamType,
    ) -> Result<StreamDefinition> {
        self.defs
            .lock()
            .iter()
            .find(|d| &d.org_id == org_id && d.name == name && d.stream_type == stream_type)
            .cloned()
            .ok_or_else(|| Error::not_found("stream"))
    }
    async fn list(&self, org_id: &Id) -> Result<Vec<StreamDefinition>> {
        Ok(self
            .defs
            .lock()
            .iter()
            .filter(|d| &d.org_id == org_id)
            .cloned()
            .collect())
    }
    async fn delete(&self, _id: &Id) -> Result<()> {
        unimplemented!()
    }
}

struct MemUserRepo {
    users: Mutex<Vec<User>>,
}

#[async_trait]
impl UserRepository for MemUserRepo {
    async fn create(&self, user: User) -> Result<User> {
        self.users.lock().push(user.clone());
        Ok(user)
    }
    async fn get(&self, id: &Id) -> Result<User> {
        self.users
            .lock()
            .iter()
            .find(|u| &u.id == id)
            .cloned()
            .ok_or_else(|| Error::not_found("user"))
    }
    async fn get_by_email(&self, email: &str) -> Result<User> {
        self.users
            .lock()
            .iter()
            .find(|u| u.email == email)
            .cloned()
            .ok_or_else(|| Error::not_found("user"))
    }
    async fn update(&self, _user: User) -> Result<User> {
        unimplemented!()
    }
    async fn delete(&self, _id: &Id) -> Result<()> {
        unimplemented!()
    }
    async fn count(&self) -> Result<u64> {
        Ok(self.users.lock().len() as u64)
    }
    async fn list(&self) -> Result<Vec<User>> {
        Ok(self.users.lock().clone())
    }
    async fn set_status(&self, id: &Id, status: molesignal::domain::iam::UserStatus) -> Result<()> {
        if let Some(u) = self.users.lock().iter_mut().find(|u| &u.id == id) {
            u.status = status;
        }
        Ok(())
    }
}

struct MemOrgRepo {
    orgs: Mutex<Vec<Organization>>,
}

#[async_trait]
impl OrganizationRepository for MemOrgRepo {
    async fn create(&self, org: Organization) -> Result<Organization> {
        self.orgs.lock().push(org.clone());
        Ok(org)
    }
    async fn get(&self, id: &Id) -> Result<Organization> {
        self.orgs
            .lock()
            .iter()
            .find(|o| &o.id == id)
            .cloned()
            .ok_or_else(|| Error::not_found("org"))
    }
    async fn get_by_slug(&self, slug: &str) -> Result<Organization> {
        self.orgs
            .lock()
            .iter()
            .find(|o| o.slug == slug)
            .cloned()
            .ok_or_else(|| Error::not_found("org"))
    }
    async fn list(&self) -> Result<Vec<Organization>> {
        Ok(self.orgs.lock().clone())
    }
    async fn update_name(&self, _id: &Id, _name: String) -> Result<Organization> {
        unimplemented!()
    }
    async fn set_disabled(&self, _id: &Id, _disabled: bool) -> Result<Organization> {
        unimplemented!()
    }
    async fn delete(&self, _id: &Id) -> Result<()> {
        unimplemented!()
    }
}

struct MemMembershipRepo {
    rows: Mutex<Vec<IamMembership>>,
}

#[async_trait]
impl IamMembershipRepository for MemMembershipRepo {
    async fn upsert(&self, m: IamMembership, _role_ids: &[Id], _actor_id: &Id) -> Result<()> {
        self.rows.lock().push(m);
        Ok(())
    }
    async fn list_for_user(&self, user_id: &Id) -> Result<Vec<IamMembership>> {
        Ok(self
            .rows
            .lock()
            .iter()
            .filter(|m| &m.user_id == user_id)
            .cloned()
            .collect())
    }
    async fn list_for_org(&self, _org_id: &Id) -> Result<Vec<IamMembership>> {
        unimplemented!()
    }
    async fn assigned_roles(&self, _user_id: &Id, _org_id: &Id) -> Result<Vec<IamAssignedRole>> {
        Ok(Vec::new())
    }
    async fn role_id_for_purpose(&self, org_id: &Id, purpose: &str) -> Result<Id> {
        Ok(Id::from_string(format!("{}-{purpose}", org_id.0)))
    }
    async fn remove(&self, _user_id: &Id, _org_id: &Id) -> Result<()> {
        unimplemented!()
    }
}

/// Flight SQL 不承载 PromQL；测试里给一个永远报错的占位实现。
struct NoPromql;

#[async_trait]
impl PromqlEngine for NoPromql {
    async fn execute(&self, _req: QueryRequest) -> Result<QueryResult> {
        Err(Error::invalid("promql not supported in this test"))
    }
}

/// 慢 SQL 引擎：用于 registry 可见性 + cancel 测试。
struct SlowEngine;

#[async_trait]
impl QueryEngine for SlowEngine {
    async fn execute(&self, _req: QueryRequest) -> Result<QueryResult> {
        tokio::time::sleep(Duration::from_millis(800)).await;
        Ok(QueryResult {
            columns: vec!["x".into()],
            rows: Vec::new(),
            scanned_rows: 0,
            took_ms: 0,
            federation: None,
        })
    }
}

// === fixture assembly ===

fn sample_stream(org: &Id) -> StreamDefinition {
    StreamDefinition {
        id: Id::new(),
        org_id: org.clone(),
        name: "app".into(),
        stream_type: StreamType::Logs,
        schema: Schema {
            fields: vec![
                FieldDef {
                    name: "level".into(),
                    data_type: FieldType::Utf8,
                    nullable: false,
                    indexed: true,
                    encrypted: false,
                    exact: false,
                },
                FieldDef {
                    name: "latency_ms".into(),
                    data_type: FieldType::Int64,
                    nullable: true,
                    indexed: true,
                    encrypted: false,
                    exact: false,
                },
            ],
        },
        retention: Some(Retention { days: 30 }),
        created_at: TimestampMicros::now(),
        updated_at: TimestampMicros::now(),
    }
}

/// 时间戳取 now 附近 —— Flight SQL 的缺省回看窗口是 `now - 24h .. now`。
fn sample_batch(stream: &StreamDefinition) -> RecordBatch {
    let schema = to_arrow(&stream.schema);
    let base = TimestampMicros::now().0;
    let ts =
        TimestampMicrosecondArray::from(vec![base - 3_000_000, base - 2_000_000, base - 1_000_000])
            .with_timezone("UTC");
    let level = StringArray::from(vec!["info", "warn", "error"]);
    let latency = Int64Array::from(vec![Some(10), Some(20), Some(30)]);
    RecordBatch::try_new(
        schema,
        vec![Arc::new(ts), Arc::new(level), Arc::new(latency)],
    )
    .unwrap()
}

struct TestServer {
    addr: std::net::SocketAddr,
    query: Arc<QueryService>,
    org: Id,
    /// 完整 `ms_*` 明文 token。
    token: String,
    /// 账号密码登录用的测试用户（属于 org 与 org2，org 在前）。
    email: String,
    password: String,
    /// 第二个 org 的 slug（streams 仅含 `other`）。
    org2_slug: String,
}

/// 铸一个 viewer token（viewer 具备 StreamRead）写进 repo，返回明文。
async fn mint_token(repo: &MemApiTokenRepo, org: &Id) -> String {
    let (prefix, secret) = generate_token_parts();
    let plaintext = assemble_token(&prefix, &secret);
    let now = TimestampMicros::now();
    repo.create(ApiToken {
        id: Id::new(),
        prefix,
        secret_hash: hash_secret(&secret).unwrap(),
        org_id: org.clone(),
        user_id: Id::new(),
        role_id: Id::from_string("role-flight-sql"),
        name: "it-flight-sql".into(),
        expires_at: None,
        last_used_at: None,
        revoked: false,
        created_at: now,
        is_default: false,
        token_kind: molesignal::domain::iam::api_token::ApiTokenKind::Personal,
        application_id: None,
    })
    .await
    .unwrap();
    plaintext
}

/// 起一个完整 fixture（真 parquet + DataFusion）或注入自定义 engine 的 server。
async fn start_server(engine_override: Option<Arc<dyn QueryEngine>>) -> TestServer {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    // tempdir 的生命周期交给测试进程（leak 到进程结束，避免 store 写盘后目录被删）。
    std::mem::forget(tmp);

    let org = Id::from_string("org-flight");
    let stream = sample_stream(&org);

    let files = Arc::new(MemParquetFileMetaRepo {
        files: Mutex::new(Vec::new()),
    });
    let writer = ParquetWriter::new(store.clone());
    let meta = writer.flush(&stream, sample_batch(&stream)).await.unwrap();
    files.insert(meta).await.unwrap();

    let engine: Arc<dyn QueryEngine> = engine_override
        .unwrap_or_else(|| Arc::new(DataFusionEngine::new(files.clone(), store.clone())));
    let admission = Arc::new(molesignal::app::search::AdmissionController::new(
        molesignal::app::search::AdmissionConfig::default(),
    ));
    let query = Arc::new(QueryService::new(engine, Arc::new(NoPromql), admission));

    let tokens = MemApiTokenRepo {
        tokens: Mutex::new(Vec::new()),
    };
    let token = mint_token(&tokens, &org).await;

    // 第二个 org：仅元数据（stream `other`），用于 org 选择子测试。
    let org2 = Id::from_string("org-second");
    let mut other = sample_stream(&org2);
    other.name = "other".into();
    let streams = MemStreamRepo {
        defs: Mutex::new(vec![stream, other]),
    };

    // 账号密码登录：alice 属于 org（在前，即缺省 org）与 org2。
    let email = "alice@example.com".to_string();
    let password = "hunter2-it!".to_string();
    let alice = User {
        id: Id::new(),
        email: email.clone(),
        display_name: "Alice".into(),
        avatar_url: None,
        bio: String::new(),
        password_hash: hash_password(&password).unwrap(),
        disabled: false,
        status: molesignal::domain::iam::UserStatus::Active,
        created_at: TimestampMicros::now(),
    };
    let users = MemUserRepo {
        users: Mutex::new(vec![alice.clone()]),
    };
    let orgs = MemOrgRepo {
        orgs: Mutex::new(vec![
            Organization {
                id: org.clone(),
                name: "Flight Org".into(),
                slug: "flight".into(),
                system: false,
                disabled: false,
                created_at: TimestampMicros::now(),
            },
            Organization {
                id: org2.clone(),
                name: "Second Org".into(),
                slug: "second".into(),
                system: false,
                disabled: false,
                created_at: TimestampMicros::now(),
            },
        ]),
    };
    let memberships = MemMembershipRepo {
        rows: Mutex::new(vec![
            IamMembership {
                user_id: alice.id.clone(),
                org_id: org.clone(),
                joined_at: TimestampMicros::now(),
            },
            IamMembership {
                user_id: alice.id.clone(),
                org_id: org2.clone(),
                joined_at: TimestampMicros::now(),
            },
        ]),
    };
    let iam = Arc::new(IamService::new(
        Arc::new(users),
        Arc::new(orgs),
        Arc::new(memberships),
        AuthSettings::default(),
        vec![b"it-flight-sql-secret".to_vec()],
    ));

    let settings = FlightSqlSettings {
        enabled: true,
        bind: "127.0.0.1".into(),
        port: 0,
        default_lookback_hours: 24,
        max_message_size_mb: 32,
    };
    let svc = FlightSqlGrpc::new(
        query.clone(),
        Arc::new(tokens),
        Arc::new(streams),
        iam,
        Arc::new(TestIamContextEnricher),
        settings,
    )
    .into_server();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(svc)
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .unwrap();
    });

    TestServer {
        addr,
        query,
        org,
        token,
        email,
        password,
        org2_slug: "second".into(),
    }
}

/// do_get 一个返回表清单的 FlightInfo，取出 `table_name` 列。
async fn fetch_table_names(
    client: &mut FlightSqlServiceClient<Channel>,
    info: arrow_flight::FlightInfo,
) -> Vec<String> {
    let ticket = info.endpoint[0].ticket.clone().expect("ticket");
    let batches: Vec<RecordBatch> = client
        .do_get(ticket)
        .await
        .expect("do_get tables")
        .try_collect()
        .await
        .expect("collect tables");
    batches
        .iter()
        .flat_map(|b| {
            let col = b
                .column_by_name("table_name")
                .expect("table_name column")
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            (0..col.len()).map(|i| col.value(i).to_string())
        })
        .collect()
}

fn all_tables_cmd() -> CommandGetTables {
    CommandGetTables {
        catalog: None,
        db_schema_filter_pattern: None,
        table_name_filter_pattern: None,
        table_types: Vec::new(),
        include_schema: false,
    }
}

async fn connect(addr: std::net::SocketAddr) -> FlightSqlServiceClient<Channel> {
    let channel = Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .unwrap();
    FlightSqlServiceClient::new(channel)
}

// === tests ===

#[tokio::test]
async fn handshake_get_tables_and_select_round_trip() {
    let server = start_server(None).await;
    let mut client = connect(server.addr).await;

    // Handshake：username 任意，password = ms_ token；client 自动把响应头里的
    // bearer 设为后续请求的 token。
    client
        .handshake("token", &server.token)
        .await
        .expect("handshake should succeed");

    // get_tables：org 下唯一 stream `app`，schema 列 = stream_type。
    let info = client
        .get_tables(CommandGetTables {
            catalog: None,
            db_schema_filter_pattern: None,
            table_name_filter_pattern: None,
            table_types: Vec::new(),
            include_schema: false,
        })
        .await
        .expect("get_tables");
    let ticket = info.endpoint[0].ticket.clone().expect("tables ticket");
    let batches: Vec<RecordBatch> = client
        .do_get(ticket)
        .await
        .expect("do_get tables")
        .try_collect()
        .await
        .expect("collect tables");
    let names: Vec<String> = batches
        .iter()
        .flat_map(|b| {
            let col = b
                .column_by_name("table_name")
                .expect("table_name column")
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            (0..col.len()).map(|i| col.value(i).to_string())
        })
        .collect();
    assert_eq!(names, vec!["app"]);
    let schemas: Vec<String> = batches
        .iter()
        .flat_map(|b| {
            let col = b
                .column_by_name("db_schema_name")
                .expect("db_schema_name column")
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            (0..col.len()).map(|i| col.value(i).to_string())
        })
        .collect();
    assert_eq!(schemas, vec!["logs"]);

    // SELECT over `logs.app`（限定名 → 引擎收到裸名 `app`）。
    let info = client
        .execute(
            "SELECT level, latency_ms FROM logs.app WHERE latency_ms > 15 ORDER BY latency_ms"
                .to_string(),
            None,
        )
        .await
        .expect("execute select");
    let ticket = info.endpoint[0].ticket.clone().expect("select ticket");
    let batches: Vec<RecordBatch> = client
        .do_get(ticket)
        .await
        .expect("do_get select")
        .try_collect()
        .await
        .expect("collect select");
    let total: usize = batches.iter().map(|b| b.num_rows()).sum();
    assert_eq!(total, 2);
    let first = &batches[0];
    assert_eq!(
        first.schema().field(0).data_type(),
        &arrow::datatypes::DataType::Utf8
    );
    assert_eq!(
        first.schema().field(1).data_type(),
        &arrow::datatypes::DataType::Int64
    );
    let levels = first
        .column_by_name("level")
        .unwrap()
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    assert_eq!(levels.value(0), "warn");
    assert_eq!(levels.value(1), "error");
}

#[tokio::test]
async fn get_tables_include_schema_returns_real_columns() {
    let server = start_server(None).await;
    let mut client = connect(server.addr).await;
    client.handshake("token", &server.token).await.unwrap();

    // DBeaver 数据浏览页靠 include_schema=true 的列元数据建网格，必须是真实列。
    let info = client
        .get_tables(CommandGetTables {
            catalog: None,
            db_schema_filter_pattern: None,
            table_name_filter_pattern: Some("app".into()),
            table_types: Vec::new(),
            include_schema: true,
        })
        .await
        .expect("get_tables with schema");
    let ticket = info.endpoint[0].ticket.clone().unwrap();
    let batches: Vec<RecordBatch> = client
        .do_get(ticket)
        .await
        .expect("do_get tables")
        .try_collect()
        .await
        .expect("collect tables");
    let batch = &batches[0];
    assert_eq!(batch.num_rows(), 1);
    let schema_col = batch
        .column_by_name("table_schema")
        .expect("table_schema column")
        .as_any()
        .downcast_ref::<arrow::array::BinaryArray>()
        .unwrap();
    let ipc_bytes = schema_col.value(0);
    assert!(!ipc_bytes.is_empty(), "table schema must not be empty");
    // IPC bytes → Arrow schema，应含 _timestamp + stream 定义里的字段
    let table_schema =
        arrow::ipc::convert::try_schema_from_ipc_buffer(ipc_bytes).expect("parse ipc schema");
    let names: Vec<&str> = table_schema
        .fields()
        .iter()
        .map(|f| f.name().as_str())
        .collect();
    assert!(names.contains(&"_timestamp"), "{names:?}");
    assert!(names.contains(&"level"), "{names:?}");
    assert!(names.contains(&"latency_ms"), "{names:?}");
}

#[tokio::test]
async fn constraint_metadata_returns_empty_not_unimplemented() {
    use arrow_flight::sql::{CommandGetImportedKeys, CommandGetPrimaryKeys};

    let server = start_server(None).await;
    let mut client = connect(server.addr).await;
    client.handshake("token", &server.token).await.unwrap();

    // DBeaver 打开表时会查约束：必须返回 0 行而非 Unimplemented 弹错。
    let info = client
        .get_primary_keys(CommandGetPrimaryKeys {
            catalog: None,
            db_schema: Some("logs".into()),
            table: "app".into(),
        })
        .await
        .expect("get_primary_keys");
    let batches: Vec<RecordBatch> = client
        .do_get(info.endpoint[0].ticket.clone().unwrap())
        .await
        .expect("do_get primary keys")
        .try_collect()
        .await
        .expect("collect primary keys");
    assert_eq!(batches.iter().map(|b| b.num_rows()).sum::<usize>(), 0);

    let info = client
        .get_imported_keys(CommandGetImportedKeys {
            catalog: None,
            db_schema: Some("logs".into()),
            table: "app".into(),
        })
        .await
        .expect("get_imported_keys");
    let batches: Vec<RecordBatch> = client
        .do_get(info.endpoint[0].ticket.clone().unwrap())
        .await
        .expect("do_get imported keys")
        .try_collect()
        .await
        .expect("collect imported keys");
    assert_eq!(batches.iter().map(|b| b.num_rows()).sum::<usize>(), 0);
}

#[tokio::test]
async fn prepared_statement_round_trip_with_nonempty_dataset_schema() {
    let server = start_server(None).await;
    let mut client = connect(server.addr).await;
    client.handshake("token", &server.token).await.unwrap();

    // JDBC 驱动用 dataset_schema 是否含字段判断 SELECT vs UPDATE：必须非空。
    let mut prepared = client
        .prepare("SELECT level FROM logs.app LIMIT 5".to_string(), None)
        .await
        .expect("create prepared statement");
    assert!(
        !prepared.dataset_schema().unwrap().fields().is_empty(),
        "dataset_schema must be non-empty or JDBC classifies the statement as UPDATE"
    );

    let info = prepared.execute().await.expect("execute prepared");
    let ticket = info.endpoint[0].ticket.clone().expect("prepared ticket");
    let batches: Vec<RecordBatch> = client
        .do_get(ticket)
        .await
        .expect("do_get prepared")
        .try_collect()
        .await
        .expect("collect prepared");
    // fixture 的 app stream 共 3 行，LIMIT 5 拿全量。
    assert_eq!(batches.iter().map(|b| b.num_rows()).sum::<usize>(), 3);
    prepared.close().await.expect("close prepared");
}

#[tokio::test]
async fn invalid_token_rejected_at_handshake_and_rpc() {
    let server = start_server(None).await;

    // handshake 用错误 token
    let mut client = connect(server.addr).await;
    let err = client
        .handshake(
            "token",
            "ms_0000000000000000_00000000000000000000000000000000",
        )
        .await
        .expect_err("bad token must fail handshake");
    assert!(err.to_string().contains("handshake"), "{err}");

    // 不 handshake、不带 token 直接查 → unauthenticated
    let mut bare = connect(server.addr).await;
    let err = bare
        .execute("SELECT 1".to_string(), None)
        .await
        .expect_err("missing bearer must fail");
    assert!(err.to_string().to_lowercase().contains("bearer"), "{err}");
}

#[tokio::test]
async fn cross_org_ticket_replay_rejected() {
    let server = start_server(None).await;
    let mut client = connect(server.addr).await;
    client.handshake("token", &server.token).await.unwrap();

    // 伪造 org B 的 statement ticket，配 org A 的 bearer 重放。
    let forged = TicketStatementQuery {
        statement_handle: FlightSqlTicket {
            sql: "SELECT * FROM logs.app".into(),
            org_id: "other-org".into(),
        }
        .encode_to_vec()
        .into(),
    };
    let err = client
        .do_get(Ticket::new(forged.as_any().encode_to_vec()))
        .await
        .expect_err("cross-org ticket must be rejected");
    assert!(err.to_string().contains("org"), "{err}");
}

#[tokio::test]
async fn dml_and_ddl_rejected() {
    let server = start_server(None).await;
    let mut client = connect(server.addr).await;
    client.handshake("token", &server.token).await.unwrap();

    for sql in ["INSERT INTO app VALUES (1)", "DROP TABLE app"] {
        let err = client
            .execute(sql.to_string(), None)
            .await
            .expect_err("write statement must be rejected");
        assert!(
            err.to_string().contains("SELECT") || err.to_string().contains("invalid"),
            "{sql}: {err}"
        );
    }
}

#[tokio::test]
async fn password_signin_round_trip_with_default_org() {
    let server = start_server(None).await;
    let mut client = connect(server.addr).await;

    // username = 邮箱，password = 账号密码 → JWT bearer，org 取第一个 membership。
    client
        .handshake(&server.email, &server.password)
        .await
        .expect("password handshake");

    // 缺省 org = org-flight：只看得到 `app`，看不到 org-second 的 `other`。
    let info = client.get_tables(all_tables_cmd()).await.expect("tables");
    assert_eq!(fetch_table_names(&mut client, info).await, vec!["app"]);

    // JWT bearer 跑真查询。
    let info = client
        .execute("SELECT count(*) AS n FROM logs.app".to_string(), None)
        .await
        .expect("execute with jwt");
    let ticket = info.endpoint[0].ticket.clone().unwrap();
    let batches: Vec<RecordBatch> = client
        .do_get(ticket)
        .await
        .expect("do_get")
        .try_collect()
        .await
        .expect("collect");
    assert_eq!(batches.iter().map(|b| b.num_rows()).sum::<usize>(), 1);
}

#[tokio::test]
async fn password_signin_org_selector_switches_org() {
    let server = start_server(None).await;

    // `<email>@<org slug>` → org-second，表清单只有 `other`。
    let mut client = connect(server.addr).await;
    let username = format!("{}@{}", server.email, server.org2_slug);
    client
        .handshake(&username, &server.password)
        .await
        .expect("org-selector handshake");
    let info = client.get_tables(all_tables_cmd()).await.expect("tables");
    assert_eq!(fetch_table_names(&mut client, info).await, vec!["other"]);

    // 不存在的 org 选择子 → 拒绝。
    let mut client = connect(server.addr).await;
    let username = format!("{}@nope", server.email);
    let err = client
        .handshake(&username, &server.password)
        .await
        .expect_err("unknown org must be rejected");
    assert!(err.to_string().contains("handshake"), "{err}");
}

#[tokio::test]
async fn wrong_password_rejected() {
    let server = start_server(None).await;
    let mut client = connect(server.addr).await;
    let err = client
        .handshake(&server.email, "not-the-password")
        .await
        .expect_err("wrong password must fail");
    assert!(err.to_string().contains("handshake"), "{err}");
}

#[tokio::test]
async fn query_visible_in_registry_and_cancellable() {
    let server = start_server(Some(Arc::new(SlowEngine))).await;
    let mut client = connect(server.addr).await;
    client.handshake("token", &server.token).await.unwrap();

    let info = client
        .execute("SELECT * FROM logs.app".to_string(), None)
        .await
        .unwrap();
    let ticket = info.endpoint[0].ticket.clone().unwrap();
    let fetch =
        tokio::spawn(async move { client.do_get(ticket).await?.try_collect::<Vec<_>>().await });

    // 轮询 registry 直到查询可见（SlowEngine 睡 800ms，窗口充足）。
    let registry = server.query.registry();
    let mut snapshot = None;
    for _ in 0..100 {
        let running = registry.list_for(Some(&server.org));
        if let Some(q) = running.first() {
            snapshot = Some(q.clone());
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let snapshot = snapshot.expect("query should appear in registry while running");
    assert_eq!(snapshot.org_id, server.org.0);
    assert!(snapshot.statement.contains("SELECT"), "{snapshot:?}");

    // cancel → run_tracked 收尾时翻 cancelled error → 客户端收 gRPC 错误。
    registry.cancel(&snapshot.id).expect("cancel");
    let result = fetch.await.expect("join");
    let err = result.expect_err("cancelled query must surface an error");
    assert!(
        err.to_string().to_lowercase().contains("cancel"),
        "expected cancel error, got: {err}"
    );
}
