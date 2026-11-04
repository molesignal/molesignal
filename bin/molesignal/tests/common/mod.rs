// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 集成测试 fixture：起 postgres testcontainer → tempdir 当 object_store →
//! 调 `bootstrap::build_state` → 把 axum router 跑在随机端口。
//!
//! 测试默认通过 `MS_RUN_IT=1` 才跑（避免本地无 docker 时单测红）。没有 docker 的机器再设
//! `MS_IT_POSTGRES_DSN`，见 [`test_database`]。

// 公共 fixture 的字段对不同 it_*.rs 的可见性不一致是预期内的。
#![allow(dead_code, clippy::field_reassign_with_default)]

use std::sync::Arc;

use arrow::array::RecordBatch;
use molesignal::{
    api::http,
    config::{
        AuthSettings, MetaDatabaseSettings, NotifySettings, ObjectStoreSettings, ProbeGrpcSettings,
        Settings,
    },
    domain::{
        iam::{
            IamMembership, IamMembershipRepository, IamScope, Organization, OrganizationRepository,
        },
        storage::{
            ArtifactRole, ArtifactState, DatasetState, PhysicalDataset, PhysicalDatasetId,
            QueryFile, primary_dataset_type, type_id,
        },
        stream::StreamDefinition,
    },
    infra::storage::parquet::writer::ParquetWriter,
    shared::{ids::Id, time::TimestampMicros},
};
use testcontainers::{ContainerAsync, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres as PgImage;

/// One test server's database: a new one on the Postgres `MS_IT_POSTGRES_DSN` names, or a container.
/// The role has to be able to create databases. The new databases stay behind, named `ms_it_*`, for
/// whoever set the variable to drop after the run.
pub async fn test_database() -> (String, Option<ContainerAsync<PgImage>>) {
    if let Ok(server) = std::env::var("MS_IT_POSTGRES_DSN") {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

        let name = format!(
            "ms_it_{}_{}",
            TimestampMicros::now().0,
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );

        let admin = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&server)
            .await
            .expect("connect to MS_IT_POSTGRES_DSN");

        sqlx::query(&format!("CREATE DATABASE {name}"))
            .execute(&admin)
            .await
            .expect("create the database of a test server");

        admin.close().await;

        let (address, _) = server
            .rsplit_once('/')
            .expect("MS_IT_POSTGRES_DSN names a database");

        return (format!("{address}/{name}"), None);
    }

    let pg = PgImage::default()
        .start()
        .await
        .expect("start postgres container");
    let port = pg.get_host_port_ipv4(5432).await.expect("pg port");
    let host = pg.get_host().await.expect("pg host");

    (
        format!("postgres://postgres:postgres@{host}:{port}/postgres"),
        Some(pg),
    )
}

pub struct TestServer {
    pub base_url: String,
    pub root_user_id: Id,
    pub root_org_id: Id,
    pub root_token: String,
    pub root_email: String,
    pub root_password: String,
    pub client: reqwest::Client,
    pub state: molesignal::api::AppState,
    pub settings: Settings,
    pub object_store_root: tempfile::TempDir,
    _logger_guard: Option<molesignal::shared::telemetry::LoggerGuard>,
    _pg: Option<ContainerAsync<PgImage>>,
    _server: tokio::task::JoinHandle<()>,
    _wal_root: tempfile::TempDir,
}

impl TestServer {
    pub async fn start() -> Self {
        Self::start_inner(false, false, None, |_| {}).await
    }

    /// 独立 integration-test binary 使用的完整自遥测启动路径。该模式安装全局
    /// self-telemetry subscriber，并把回灌目标固定到 `_sys/_molesignal`。
    pub async fn start_with_trace_capture() -> Self {
        Self::start_inner(true, false, None, |_| {}).await
    }

    /// 缩短 APM flush 周期，供完整摄取到查询链路测试使用。
    pub async fn start_with_apm() -> Self {
        Self::start_inner(false, true, None, |_| {}).await
    }

    /// Probe endpoints on these settings, for a test that serves them with `serve_probe_grpc`:
    /// the endpoints an Agent is told to use are fixed while the state is built.
    pub async fn start_with_probe(probe: ProbeGrpcSettings) -> Self {
        Self::start_inner(false, false, Some(probe), |_| {}).await
    }

    /// 写入一秒内落盘，供写入后马上读 Catalog 或查询的测试使用；默认周期是 30 秒。
    pub async fn start_with_fast_flush() -> Self {
        Self::start_inner(false, false, None, |settings| {
            settings.intake.flush_interval_secs = 1;
        })
        .await
    }

    /// 写入一秒内落盘，链路一条不丢，根 Span 到了很快就判定；默认只留一成，也判定得慢。
    pub async fn start_keeping_traces() -> Self {
        Self::start_inner(false, false, None, |settings| {
            settings.intake.flush_interval_secs = 1;
            settings.telemetry.trace.deployment_environment = "test".into();
            settings.telemetry.trace.root_grace_millis = 50;
        })
        .await
    }

    /// [`Self::start_with_trace_capture`] 之外再归档服务自身的 Profile，默认关闭。
    pub async fn start_with_self_profiles() -> Self {
        Self::start_inner(true, false, None, |settings| {
            settings.telemetry.self_collect.profiles_enabled = true;
        })
        .await
    }

    /// 服务照常捕获自身的链路，但 [`Self::start_with_trace_capture`] 打开的自遥测总开关关着。
    pub async fn start_with_self_telemetry_off() -> Self {
        Self::start_inner(true, false, None, |settings| {
            settings.telemetry.self_collect.enabled = false;
        })
        .await
    }

    async fn start_inner(
        capture_traces: bool,
        accelerate_apm_flush: bool,
        probe: Option<ProbeGrpcSettings>,
        adjust: impl FnOnce(&mut Settings),
    ) -> Self {
        // 全局 config 单例必须先 init，否则 `/api/v1/query` 等 handler 调
        // `molesignal::config::get()` 会 panic。tests/common 内只 init 一次（OnceCell）。
        let _ = molesignal::config::load(None);

        let (dsn, pg) = test_database().await;

        let object_root = tempfile::tempdir().expect("tmpdir for object_store");
        let wal_root = tempfile::tempdir().expect("tmpdir for WAL");

        let mut settings = Settings::default();

        // 默认的 ./data/wal 落在 crate 目录里，还被进程里所有 TestServer 共用。
        settings.wal.dir = wal_root.path().to_string_lossy().into();

        settings.meta_database = MetaDatabaseSettings {
            dsn: dsn.clone(),
            min_connections: 1,
            max_connections: 5,
        };
        settings.object_store = ObjectStoreSettings {
            backend: "local".into(),
            root: object_root.path().to_string_lossy().into(),
            ..Default::default()
        };
        settings.auth = AuthSettings {
            deprecated_jwt_secret: None,
            session_ttl_secs: 3600,
            // 留空让我们手动建用户（更可控）
            root_email: String::new(),
            root_password: String::new(),
        };
        settings.notify = NotifySettings::default();
        settings.http.bind = "127.0.0.1".into();
        settings.http.port = 0; // 随机端口
        settings.http.external_url = "http://127.0.0.1".into();
        if accelerate_apm_flush {
            settings.apm.flush_interval_ms = 100;
        }
        if let Some(probe) = probe {
            settings.probe = probe;
        }
        if capture_traces {
            settings.intake.flush_interval_secs = 1;
            settings.telemetry.log_level = "info".into();
            settings.telemetry.trace.filter = "info".into();
            settings.telemetry.trace.deployment_environment = "test".into();
            settings.telemetry.trace.root_grace_millis = 50;
            settings.telemetry.self_collect.enabled = true;
            settings.telemetry.self_collect.metrics_enabled = true;
            settings.telemetry.self_collect.metrics_interval_secs = 1;
            settings.telemetry.self_collect.batch_max_delay_ms = 20;
        }

        adjust(&mut settings);

        let logger_guard = if capture_traces {
            use molesignal::shared::{
                self_telemetry::{ResourceIdentity, SelfTelemetryInit},
                telemetry::{FullTelemetryInit, LogFormat, LogOutput},
            };
            let guard =
                molesignal::shared::telemetry::init_full_with_self_telemetry(FullTelemetryInit {
                    log_level: &settings.telemetry.log_level,
                    format: LogFormat::Text,
                    output: LogOutput::Console,
                    trace_filter: &settings.telemetry.trace.filter,
                    self_telemetry: Some(SelfTelemetryInit {
                        queue_capacity: settings.telemetry.self_collect.queue_capacity,
                        traces_enabled: true,
                        resource: ResourceIdentity::new(
                            "molesignal",
                            env!("CARGO_PKG_VERSION"),
                            "test",
                            "standalone",
                            "trace-e2e-node",
                        ),
                    }),
                })
                .expect("initialize traced integration-test subscriber");
            Some(guard)
        } else {
            // 测试期间把 server 端 tracing 打到 stderr，方便 `--nocapture` 下定位
            // handler 抛 Internal 时被 sanitize 的真因。`try_init` 多次调失败即视为
            // 已被另一测试初始化过。`RUST_LOG=debug` 可覆盖。
            let _ = tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                        tracing_subscriber::EnvFilter::new("info,molesignal=debug")
                    }),
                )
                .with_writer(std::io::stderr)
                .try_init();
            None
        };

        // 静态化 settings：bootstrap::build_state 需要 `&Settings`，配 'static lifetime
        let settings_static: &'static Settings = Box::leak(Box::new(settings.clone()));
        let mut state = molesignal::bootstrap::build_state(settings_static)
            .await
            .expect("build_state");
        if let Some(hub) = logger_guard
            .as_ref()
            .and_then(molesignal::shared::telemetry::LoggerGuard::self_telemetry)
        {
            molesignal::bootstrap::activate_self_telemetry(&mut state, settings_static, hub)
                .expect("activate traced integration-test self telemetry");
        }

        // 手动建 root org / user：避免对 settings.auth.root_* 路径耦合
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&dsn)
            .await
            .expect("test pool");
        let orgs = molesignal::infra::persistence::repositories::organizations::PgOrganizationRepository::new(
            pool.clone(),
        );
        let memberships =
            molesignal::infra::persistence::repositories::iam::memberships::PgIamMembershipRepository::new(
                pool.clone(),
            );

        let org = orgs
            .create(Organization {
                id: Id::new(),
                name: "Acme".into(),
                slug: "acme".into(),
                system: false,
                disabled: false,
                created_at: TimestampMicros::now(),
            })
            .await
            .expect("create org");

        let email = "root@test.example".to_string();
        let password = "rootpass".to_string();
        let user = state
            .iam
            .service
            .create_user(email.clone(), "root".into(), &password)
            .await
            .expect("create root user");
        let bootstrap_role_id = memberships
            .role_id_for_purpose(&org.id, "organization_bootstrap")
            .await
            .expect("resolve bootstrap IAM role");
        memberships
            .upsert(
                IamMembership {
                    user_id: user.id.clone(),
                    org_id: org.id.clone(),
                    joined_at: TimestampMicros::now(),
                },
                &[bootstrap_role_id],
                &user.id,
            )
            .await
            .expect("upsert root membership");
        let token = state
            .iam
            .service
            .issue_token(&user.id, &org.id)
            .expect("issue root token");

        // 起 axum
        let app = http::build_router(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind 127.0.0.1:0");
        let actual_addr = listener.local_addr().expect("local_addr");
        let base_url = format!("http://{actual_addr}");
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        // 等 server 真正 ready（轮询 healthz）
        let client = reqwest::Client::new();
        for _ in 0..50 {
            if client
                .get(format!("{base_url}/api/v1/healthz"))
                .send()
                .await
                .map(|r| r.status().is_success())
                .unwrap_or(false)
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        Self {
            base_url,
            root_user_id: user.id,
            root_org_id: org.id,
            root_token: token,
            root_email: email,
            root_password: password,
            client,
            state,
            settings,
            object_store_root: object_root,
            _logger_guard: logger_guard,
            _pg: pg,
            _server: server,
            _wal_root: wal_root,
        }
    }

    /// root 登录会话的 Cookie 请求头。
    pub fn auth_header(&self) -> (&'static str, String) {
        ("cookie", format!("accessToken={}", self.root_token))
    }

    /// 执行 Mole Agent Run 的后台任务。节点启动时才起它，`build_state` 不管，要得到回答的测试
    /// 自己起，并拿住返回值直到测试结束。
    pub fn spawn_agent_runs(&self) -> Vec<tokio::task::JoinHandle<()>> {
        molesignal::bootstrap::workers::agent_runs::AgentRunWorker::new(
            self.state.clone(),
            &self.settings,
        )
        .spawn()
    }

    /// 发一条 Mole Agent 消息，等这一轮 Run 结束，返回它的事件（SSE）。途中要确认的工具一律同意，
    /// 和提问人在对话里点同意一样。要先有 [`Self::spawn_agent_runs`]。
    pub async fn ask_agent(&self, chat_id: &str, message: &serde_json::Value) -> String {
        let (name, value) = self.auth_header();

        let queued = self
            .client
            .post(format!(
                "{}/api/v1/agent/chat/{chat_id}/messages",
                self.base_url
            ))
            .header(name, &value)
            .json(message)
            .send()
            .await
            .expect("post a message");

        assert_eq!(queued.status(), 202, "the message is queued");

        let queued: serde_json::Value = queued.json().await.expect("queued run JSON");
        let run_id = queued["run"]["id"].as_str().expect("queued run id");
        let run_url = format!("{}/api/v1/agent/runs/{run_id}", self.base_url);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);

        loop {
            let run: serde_json::Value = self
                .client
                .get(&run_url)
                .header(name, &value)
                .send()
                .await
                .expect("read the run")
                .json()
                .await
                .expect("run JSON");

            if !run["finishedAt"].is_null() {
                break;
            }

            if run["pending"]["type"] == "confirmation" {
                self.client
                    .post(format!("{run_url}/respond"))
                    .header(name, &value)
                    .json(&serde_json::json!({
                        "callId": run["pending"]["callId"],
                        "approved": true,
                    }))
                    .send()
                    .await
                    .expect("confirm the tool")
                    .error_for_status()
                    .expect("the confirmation is taken");
            }

            assert!(
                std::time::Instant::now() < deadline,
                "the run did not end in time: {run}"
            );

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        self.client
            .get(format!("{run_url}/events"))
            .header(name, &value)
            .send()
            .await
            .expect("read the run's events")
            .text()
            .await
            .expect("the run's events")
    }

    /// 安全的 unused-warning anchor。
    #[allow(dead_code)]
    pub fn anchor(&self) -> Arc<()> {
        Arc::new(())
    }
}

/// 请求带上凭据：API token 放进 Authorization，登录会话的 access token 像浏览器一样放进 Cookie。
pub trait Credential {
    fn credential(self, token: impl AsRef<str>) -> Self;
}

impl Credential for reqwest::RequestBuilder {
    fn credential(self, token: impl AsRef<str>) -> Self {
        let token = token.as_ref();

        if molesignal::domain::iam::api_token::has_token_marker(token) {
            self.bearer_auth(token)
        } else {
            self.header(reqwest::header::COOKIE, format!("accessToken={token}"))
        }
    }
}

/// 响应写下的 access token（登录、切换组织、刷新）。
pub fn session_cookie(response: &reqwest::Response) -> String {
    set_cookie(response, "accessToken").expect("the response sets the session cookie")
}

/// 响应 `Set-Cookie` 里名为 `name` 的值，删除 Cookie 时是空串。
pub fn set_cookie(response: &reqwest::Response, name: &str) -> Option<String> {
    response
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|cookie| cookie.split(';').next()?.split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value.to_owned())
}

/// 进入平台组织的登录会话的 access token，和切换到 `_sys` 时签发的一样。
pub fn system_session_token(state: &molesignal::api::AppState, user_id: &Id) -> String {
    let session = state.iam.service.begin_session(
        user_id,
        &state.iam.system_org_id,
        IamScope::System,
        false,
        molesignal::app::iam::SignInMethod::single_factor(),
    );

    state
        .iam
        .service
        .issue_access_token(&session)
        .expect("issue system session")
        .0
}

pub fn skip_unless_enabled() -> bool {
    std::env::var("MS_RUN_IT").ok().as_deref() != Some("1")
}

/// Build immutable Segment/Artifact objects through the production StorageLayout, then expose the
/// primary object as a read-only Catalog query projection for isolated integration fixtures.
pub async fn write_parquet_fixture(
    writer: &ParquetWriter,
    stream: &StreamDefinition,
    batch: RecordBatch,
) -> QueryFile {
    let dataset_type = primary_dataset_type(stream.stream_type).unwrap();
    let registry = molesignal::domain::storage::builtin_registry();
    let descriptor = registry
        .dataset_type(&stream.stream_type, &dataset_type)
        .expect("built-in raw dataset descriptor");
    let spec = descriptor.to_spec();
    let dataset = PhysicalDataset {
        id: PhysicalDatasetId::from_string(format!("fixture-{}", stream.id)),
        organization_id: stream.org_id.clone(),
        logical_stream_id: stream.id.clone(),
        dataset_type,
        dataset_type_version: spec.dataset_type_version,
        partition_policy: spec.partition_policy,
        storage_policy: spec.storage_policy,
        index_policy: spec.index_policy,
        catalog_version: 0,
        state: DatasetState::Active,
        created_at_micros: 0,
        updated_at_micros: 0,
    };
    let mut segments = writer
        .write_compaction_catalog(stream, &dataset, batch)
        .await
        .expect("write explicit fixture Artifacts");
    assert_eq!(segments.len(), 1, "fixture batch must fit one partition");
    let segment = segments.remove(0);
    assert_eq!(segment.primary.role, ArtifactRole::PrimaryData);
    assert_eq!(segment.primary.state, ArtifactState::Ready);
    assert_eq!(
        segment.primary.artifact_type.as_str(),
        type_id::builtin::ARTIFACT_PARQUET
    );
    QueryFile {
        id: segment.id.0,
        org_id: stream.org_id.clone(),
        stream: stream.name.clone(),
        stream_type: stream.stream_type,
        dataset_type: primary_dataset_type(stream.stream_type).unwrap(),
        object_key: segment.primary.object.key.0,
        checksum: Some(segment.primary.object.checksum),
        etag: segment.primary.object.etag,
        time_range: segment.time_range,
        rows: segment.row_count,
        size_bytes: segment.primary.object.size_bytes,
        min_values: segment.column_stats.min_values,
        max_values: segment.column_stats.max_values,
        value_sets: segment.column_stats.value_sets,
        address_ranges: segment.column_stats.address_ranges,
        null_counts: segment.column_stats.null_counts,
        indexed_at_micros: segment.column_stats.indexed_at_micros,
        shard: None,
    }
}

/// 轮询 predicate `f` 直到返 true 或 `secs` 秒超时（每 100ms 重试）。
/// 返回 true 表示 predicate 在限时内通过；false 表示 timeout。
///
/// 用法：替代 `tokio::time::sleep(N)` 避免 flaky timing。
#[allow(dead_code)]
pub async fn wait_until<F>(secs: u64, mut f: F) -> bool
where
    F: FnMut() -> bool,
{
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    loop {
        if f() {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

/// 异步版本的 wait_until：predicate 返 future。
#[allow(dead_code)]
pub async fn wait_until_async<F, Fut>(secs: u64, mut f: F) -> bool
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    loop {
        if f().await {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

/// 创建一个 `StreamDefinition` 写入 `state.telemetry.streams` 仓库，返回 stream 元数据。
/// 用法：测试用例需要先有 stream 才能 intake 时调它。
#[allow(dead_code)]
pub async fn seed_stream(
    state: &molesignal::api::AppState,
    org_id: &molesignal::shared::ids::Id,
    name: &str,
    stream_type: molesignal::domain::stream::StreamType,
) -> molesignal::domain::stream::StreamDefinition {
    use molesignal::domain::stream::{FieldDef, FieldType, Retention, Schema, StreamDefinition};
    let def = StreamDefinition {
        id: molesignal::shared::ids::Id::new(),
        org_id: org_id.clone(),
        name: name.into(),
        stream_type,
        schema: Schema {
            fields: vec![FieldDef {
                name: "msg".into(),
                data_type: FieldType::Utf8,
                nullable: true,
                index_type: None,
                indexed: false,
                encrypted: false,
                exact: false,
            }],
        },
        retention: Some(Retention { days: 7 }),
        created_at: molesignal::shared::time::TimestampMicros::now(),
        updated_at: molesignal::shared::time::TimestampMicros::now(),
    };
    state
        .telemetry
        .streams
        .create(def.clone())
        .await
        .expect("seed_stream: create")
}
