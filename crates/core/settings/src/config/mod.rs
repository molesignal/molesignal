// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! TOML 配置加载层。
//!
//! 加载顺序（后者覆盖前者）：
//!   1. 内置默认值
//!   2. `--config <path>` 指向的 TOML 文件
//!   3. 环境变量
//!
//! 环境变量约定：前缀 `MS_`，配置路径中的所有层级都用单下划线连接。例：
//!
//! - `MS_HTTP_PORT=5081`
//! - `MS_META_DATABASE_DSN=postgres://...`
//! - `MS_NOTIFY_SMTP_HOST=smtp.example.com`
//! - `MS_AUTH_JWT_SECRET_OVERRIDE=...`
//!
//! 加载器依据 [`Settings`] 的实际层级还原路径，因此字段名自身包含的下划线不会
//! 被误拆分。例如 `MS_CLUSTER_ADVERTISE_ADDR` 对应 `cluster.advertise_addr`。
//!
//! 全局唯一入口：`config::load(path)` → `&'static Settings`
//!
//! 数据模型按 TOML section 域拆到各子模块（`node` / `network` / `object_store` / …），
//! 全部 `pub use` 到 crate 根——对外是扁平的 `crate::config::XxxSettings`。

mod agent;
mod alerting;
mod apm;
mod auth;
mod cache;
mod cluster;
mod env;
mod features;
mod intake;
mod meta_database;
mod network;
mod node;
mod object_store;
mod query;
mod storage;
mod telemetry;
mod wal;
pub mod watcher;

use std::path::Path;

pub use agent::*;
pub use alerting::*;
pub use apm::*;
pub use auth::*;
pub use cache::*;
pub use cluster::*;
pub use features::*;
use figment::{
    Figment,
    providers::{Format, Serialized, Toml},
};
pub use intake::*;
pub use meta_database::*;
pub use network::*;
pub use node::*;
pub use object_store::*;
use once_cell::sync::OnceCell;
pub use query::*;
use serde::{Deserialize, Serialize};
pub use storage::*;
pub use telemetry::*;
pub use wal::*;

/// 顶层配置聚合：每个字段对应一个 TOML section，结构定义见同名子模块。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Settings {
    /// `[apm]`: bounded pre-sampling application-performance projection.
    #[serde(default)]
    pub apm: ApmSettings,
    #[serde(default)]
    pub node: NodeSettings,
    #[serde(default)]
    pub http: HttpSettings,
    #[serde(default)]
    pub grpc: GrpcSettings,
    /// `[probe]`: Probe Agent register/control plane with mandatory TLS identities.
    #[serde(default)]
    pub probe: ProbeGrpcSettings,
    #[serde(default)]
    pub telemetry: TelemetrySettings,
    /// `[profiling]`：节点本地 pprof listener，默认关闭。
    #[serde(default)]
    pub profiling: ProfilingSettings,
    /// `[meta_database]`：元数据库连接（PostgreSQL）。
    #[serde(default)]
    pub meta_database: MetaDatabaseSettings,
    /// `[object_store]`：对象存储后端、凭据与传输参数。
    #[serde(default)]
    pub object_store: ObjectStoreSettings,
    #[serde(default)]
    pub wal: WalSettings,
    #[serde(default)]
    pub intake: IntakeSettings,
    #[serde(default)]
    pub querier: QuerierSettings,
    #[serde(default)]
    pub search: SearchSettings,
    #[serde(default)]
    pub compactor: CompactorSettings,
    #[serde(default)]
    pub alert: AlertSettings,
    #[serde(default)]
    pub notify: NotifySettings,
    #[serde(default)]
    pub auth: AuthSettings,
    #[serde(default)]
    pub cluster: ClusterSettings,
    #[serde(default)]
    pub router: RouterSettings,
    #[serde(default)]
    pub cache: CacheSettings,
    /// DB-IP City Lite mmdb：RUM 写入按客户端 IP 查国家的数据源。
    #[serde(default)]
    pub mmdb: MmdbSettings,
    /// SearchJob worker pool。
    #[serde(default)]
    pub search_jobs: SearchJobsSettings,
    /// Function runtime（VRL 永远可用；默认二进制内置 JS；本段配置 LLM）。
    #[serde(default)]
    pub functions: FunctionsSettings,
    /// Scheduled reports (`[scheduled_reports]`)：renderer 子段配置 headless
    /// Chrome PDF/PNG 渲染资源。
    #[serde(default)]
    pub scheduled_reports: ScheduledReportsSettings,
    /// `[log_inspection]`：定时日志巡检的周期下限、数量上限、并发、超时与报告保留期。
    #[serde(default)]
    pub log_inspection: LogInspectionSettings,
    /// `[agent]`：Mole Agent 的每日 token 预算与提醒阈值。
    #[serde(default)]
    pub agent: AgentSettings,
    /// `[storage]` Catalog lifecycle, index maintenance, delayed GC, and reconciliation.
    #[serde(default)]
    pub storage: StorageSettings,
    /// `[flight_sql]`（spec `flight-sql`）：对外 Arrow Flight SQL 端口。
    /// 独立于内部 gRPC（`[grpc]`，可信网络）；默认关闭，显式 opt-in 后才监听。
    #[serde(default)]
    pub flight_sql: FlightSqlSettings,
    /// `[otlp_grpc]`：对外标准 OTLP gRPC receiver（traces/logs/metrics/profiles）。
    /// 独立于内部 gRPC（`[grpc]`），始终监听标准 :4317。
    #[serde(default)]
    pub otlp_grpc: OtlpGrpcSettings,
}

/// 跨多个 section 复用的 `bool` 默认值（`true`）。
///
/// `network` 的 true 默认值，留在 crate 根供子模块经 `super::yes` 引用。
fn yes() -> bool {
    true
}

static SETTINGS: OnceCell<Settings> = OnceCell::new();

/// 从指定 TOML 文件加载配置（环境变量可覆盖），并固化为全局单例。
pub fn load(path: Option<&Path>) -> anyhow::Result<&'static Settings> {
    let defaults = Settings::default();
    let env = env::provider(&defaults)?;
    let mut fig = Figment::from(Serialized::defaults(defaults));

    if let Some(p) = path {
        fig = fig.merge(Toml::file(p));
    }

    fig = fig.merge(env);

    let mut settings: Settings = fig.extract()?;
    settings.node.id = settings.node.resolved_id();
    settings.validate()?;

    SETTINGS
        .set(settings)
        .map_err(|_| anyhow::anyhow!("settings already initialized"))?;

    Ok(SETTINGS.get().unwrap())
}

impl Settings {
    pub fn validate(&self) -> anyhow::Result<()> {
        self.node.validate()?;
        self.apm.validate()?;
        self.telemetry.self_collect.validate()?;
        self.telemetry.trace.validate()?;
        self.profiling.validate()?;
        self.scheduled_reports.renderer.validate()?;
        self.intake.validate()?;
        self.probe.validate()?;
        self.cache.object.validate()?;
        self.object_store.validate()?;
        Ok(())
    }
}

/// 获取全局配置；必须在 `load` 之后调用。
pub fn get() -> &'static Settings {
    SETTINGS
        .get()
        .expect("config not initialized; call crate::config::load first")
}

#[cfg(test)]
mod tests;
