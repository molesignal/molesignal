// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 集成测试 fixture：起 postgres testcontainer → tempdir 当 object_store →
//! 调 `bootstrap::build_state` → 把 axum router 跑在随机端口。
//!
//! 测试默认通过 `MS_RUN_IT=1` 才跑（避免本地无 docker 时单测红）。

// 公共 fixture 的字段对不同 it_*.rs 的可见性不一致是预期内的。
#![allow(dead_code, clippy::field_reassign_with_default)]

use std::sync::Arc;

use molesignal::{
    api::http,
    config::{
        AuthSettings, MetaStoreSettings, NotifySettings, ObjectStoreSettings, Settings,
        StoreSettings,
    },
    domain::iam::{IamMembership, IamMembershipRepository, Organization, OrganizationRepository},
    shared::{ids::Id, time::TimestampMicros},
};
use testcontainers::{ContainerAsync, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres as PgImage;

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
    _pg: ContainerAsync<PgImage>,
    _server: tokio::task::JoinHandle<()>,
}

impl TestServer {
    pub async fn start() -> Self {
        Self::start_inner(false, false).await
    }

    /// 独立 integration-test binary 使用的完整自遥测启动路径。该模式安装全局
    /// self-telemetry subscriber，并把回灌目标固定到 `_sys/_molesignal`。
    pub async fn start_with_trace_capture() -> Self {
        Self::start_inner(true, false).await
    }

    /// 缩短 APM flush 周期，供完整摄取到查询链路测试使用。
    pub async fn start_with_apm() -> Self {
        Self::start_inner(false, true).await
    }

    async fn start_inner(capture_traces: bool, accelerate_apm_flush: bool) -> Self {
        // 全局 config 单例必须先 init，否则 `/api/v1/query` 等 handler 调
        // `molesignal::config::get()` 会 panic。tests/common 内只 init 一次（OnceCell）。
        let _ = molesignal::config::load(None);

        let pg = PgImage::default()
            .start()
            .await
            .expect("start postgres container");
        let port = pg.get_host_port_ipv4(5432).await.expect("pg port");
        let host = pg.get_host().await.expect("pg host");
        let dsn = format!("postgres://postgres:postgres@{host}:{port}/postgres");

        let object_root = tempfile::tempdir().expect("tmpdir for object_store");

        let mut settings = Settings::default();
        settings.store = StoreSettings {
            meta: MetaStoreSettings {
                backend: "postgres".into(),
                dsn: dsn.clone(),
                min_connections: 1,
                max_connections: 5,
            },
            object: ObjectStoreSettings {
                backend: "local".into(),
                root: object_root.path().to_string_lossy().into(),
                ..Default::default()
            },
        };
        settings.auth = AuthSettings {
            deprecated_jwt_secret: None,
            token_ttl_secs: 3600,
            // 留空让我们手动建用户（更可控）
            root_email: String::new(),
            root_password: String::new(),
        };
        settings.notify = NotifySettings::default();
        settings.http.bind = "127.0.0.1".into();
        settings.http.port = 0; // 随机端口
        if accelerate_apm_flush {
            settings.apm.flush_interval_ms = 100;
        }
        if capture_traces {
            settings.ingester.flush_interval_secs = 1;
            settings.telemetry.log_level = "info".into();
            settings.telemetry.trace.filter = "info".into();
            settings.telemetry.trace.deployment_environment = "test".into();
            settings.telemetry.trace.root_grace_millis = 50;
            settings.telemetry.self_collect.enabled = true;
            settings.telemetry.self_collect.metrics_enabled = true;
            settings.telemetry.self_collect.metrics_interval_secs = 1;
            settings.telemetry.self_collect.batch_max_delay_ms = 20;
        }

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
                        logs_enabled: true,
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
        }
    }

    pub fn auth_header(&self) -> (&'static str, String) {
        ("authorization", format!("Bearer {}", self.root_token))
    }

    /// 安全的 unused-warning anchor。
    #[allow(dead_code)]
    pub fn anchor(&self) -> Arc<()> {
        Arc::new(())
    }
}

pub fn skip_unless_enabled() -> bool {
    std::env::var("MS_RUN_IT").ok().as_deref() != Some("1")
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
/// 用法：测试用例需要先有 stream 才能 ingest 时调它。
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
