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
//! - `MS_STORE_META_DSN=postgres://...`
//! - `MS_NOTIFY_SMTP_HOST=smtp.example.com`
//! - `MS_AUTH_JWT_SECRET_OVERRIDE=...`
//!
//! 加载器依据 [`Settings`] 的实际层级还原路径，因此字段名自身包含的下划线不会
//! 被误拆分。例如 `MS_CLUSTER_ADVERTISE_ADDR` 对应 `cluster.advertise_addr`。
//!
//! 全局唯一入口：`config::load(path)` → `&'static Settings`
//!
//! 数据模型按 TOML section 域拆到各子模块（`node` / `network` / `store` / …），
//! 全部 `pub use` 到 crate 根——对外是扁平的 `crate::config::XxxSettings`。

mod alerting;
mod apm;
mod auth;
mod cache;
mod cluster;
mod env;
mod features;
mod ingester;
mod license;
mod network;
mod node;
mod query;
mod storage;
mod store;
mod telemetry;
mod wal;
pub mod watcher;

use std::path::Path;

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
pub use ingester::*;
pub use license::*;
pub use network::*;
pub use node::*;
use once_cell::sync::OnceCell;
pub use query::*;
use serde::{Deserialize, Serialize};
pub use storage::*;
pub use store::*;
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
    #[serde(default)]
    pub telemetry: TelemetrySettings,
    /// `[profiling]`：节点本地 pprof listener，默认关闭。
    #[serde(default)]
    pub profiling: ProfilingSettings,
    /// 元数据库 + 对象存储统一在 `[store]` 段下（`store.meta` / `store.object`）。
    #[serde(default)]
    pub store: StoreSettings,
    #[serde(default)]
    pub wal: WalSettings,
    #[serde(default)]
    pub ingester: IngesterSettings,
    #[serde(default)]
    pub querier: QuerierSettings,
    #[serde(default)]
    pub search: SearchSettings,
    #[serde(default)]
    pub compactor: CompactorSettings,
    #[serde(default)]
    pub alert_manager: AlertManagerSettings,
    #[serde(default)]
    pub notify: NotifySettings,
    #[serde(default)]
    pub auth: AuthSettings,
    /// `[license]`：仅控制首次导入/灾备来源；License 内容永不进入配置。
    #[serde(default)]
    pub license: LicenseSettings,
    #[serde(default)]
    pub cluster: ClusterSettings,
    #[serde(default)]
    pub router: RouterSettings,
    #[serde(default)]
    pub cache: CacheSettings,
    /// MaxMind GeoLite2 mmdb：geoip_lookup 的数据源。
    #[serde(default)]
    pub mmdb: MmdbSettings,
    /// SearchJob worker pool。
    #[serde(default)]
    pub search_jobs: SearchJobsSettings,
    /// Function runtime（VRL 永远可用；JS 走 `--features js-runtime` + 本段开关）。
    #[serde(default)]
    pub functions: FunctionsSettings,
    /// Scheduled reports (`[scheduled_reports]`)：renderer 子段配置 headless
    /// Chrome PDF/PNG 渲染资源。
    #[serde(default)]
    pub scheduled_reports: ScheduledReportsSettings,
    /// Intelligence chat（`[intelligence]`）：提供默认 provider 提示。功能可用性由
    /// License 决定。API key / base URL 走 env var
    /// （`MS_INTELLIGENCE_<PROVIDER>_API_KEY` / `_BASE_URL`），不进 TOML。
    #[serde(default)]
    pub intelligence: IntelligenceSettings,
    /// `[storage]` 段：与 [`store`] 解耦的存储层子能力（spec `storage` capability）。
    /// 目前仅含 `parquet_file_meta_dump` 子段（ParquetFileMeta 冷分区下沉到 object_store）。
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
/// 由 `network`（`http.gzip`）与 `storage`（`parquet_file_meta_dump.enabled`）共享，
/// 故留在 crate 根、经 `super::yes` 引用。
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

    let settings: Settings = fig.extract()?;
    settings.validate()?;
    SETTINGS
        .set(settings)
        .map_err(|_| anyhow::anyhow!("settings already initialized"))?;
    Ok(SETTINGS.get().unwrap())
}

impl Settings {
    pub fn validate(&self) -> anyhow::Result<()> {
        self.apm.validate()?;
        self.telemetry.self_collect.validate()?;
        self.telemetry.trace.validate()?;
        self.profiling.validate()?;
        self.scheduled_reports.renderer.validate()?;
        self.ingester.validate()?;
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
