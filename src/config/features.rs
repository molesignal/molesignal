// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 可选功能与集成：`[functions]`（VRL/JS/LLM 运行时）、`[mmdb]`（GeoIP）、
//! `[scheduled_reports]`（headless 渲染）、`[intelligence]`（AI chat 默认配置）。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FunctionsSettings {
    /// LLM 评估节点开关。默认 false：`language = llm` 的 pipeline 步骤会被拒
    /// （`llm eval runtime disabled`）。开启后，每条命中事件都会触发一次模型调用 ——
    /// **成本与延迟随 ingest 量线性放大**，请仅在低吞吐流上启用，并先配好 org 的 AI provider。
    #[serde(default)]
    pub llm_eval_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MmdbSettings {
    /// MaxMind GeoLite2 license key；为空时不下载；若 db_path 文件已随包存在仍可用于 GeoIP。
    #[serde(default)]
    pub license_key: String,
    /// 本地 mmdb 文件路径。
    #[serde(default = "default_mmdb_path")]
    pub db_path: String,
    /// 周期 refresh 间隔（秒）；默认 7 天。
    #[serde(default = "default_mmdb_refresh")]
    pub refresh_interval_secs: u64,
}
fn default_mmdb_path() -> String {
    "/usr/share/molesignal/mmdb/GeoLite2-City.mmdb".into()
}
fn default_mmdb_refresh() -> u64 {
    7 * 24 * 3600
}
impl Default for MmdbSettings {
    fn default() -> Self {
        Self {
            license_key: String::new(),
            db_path: default_mmdb_path(),
            refresh_interval_secs: default_mmdb_refresh(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScheduledReportsSettings {
    #[serde(default)]
    pub renderer: RendererSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RendererSettings {
    /// Chrome 加载 dashboard/saved-view SPA 的内部基地址。
    #[serde(default = "default_renderer_base_url")]
    pub base_url: String,
    /// Chrome instance 池大小；硬上限 4（ crate 内 clamp）。
    #[serde(default = "default_concurrent_renders")]
    pub concurrent_renders: u32,
    /// 单次 render wall-clock 上限；硬上限 60s。
    #[serde(default = "default_render_timeout_secs")]
    pub render_timeout_secs: u32,
    /// Viewport 宽（像素）。
    #[serde(default = "default_viewport_width")]
    pub viewport_width: u32,
    /// Viewport 高（像素）。
    #[serde(default = "default_viewport_height")]
    pub viewport_height: u32,
}

fn default_concurrent_renders() -> u32 {
    2
}
fn default_renderer_base_url() -> String {
    "http://127.0.0.1:5173".into()
}
fn default_render_timeout_secs() -> u32 {
    30
}
fn default_viewport_width() -> u32 {
    1280
}
fn default_viewport_height() -> u32 {
    800
}

impl Default for RendererSettings {
    fn default() -> Self {
        Self {
            base_url: default_renderer_base_url(),
            concurrent_renders: default_concurrent_renders(),
            render_timeout_secs: default_render_timeout_secs(),
            viewport_width: default_viewport_width(),
            viewport_height: default_viewport_height(),
        }
    }
}

impl RendererSettings {
    pub fn validate(&self) -> anyhow::Result<()> {
        let url = url::Url::parse(&self.base_url).map_err(|error| {
            anyhow::anyhow!("invalid scheduled_reports.renderer.base_url: {error}")
        })?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            anyhow::bail!(
                "scheduled_reports.renderer.base_url must be an absolute http or https URL"
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntelligenceSettings {
    /// session.provider 为空时的 fallback；未来可用，当前仅文档化。
    #[serde(default = "default_intelligence_provider")]
    pub default_provider: String,
}

fn default_intelligence_provider() -> String {
    "openai".into()
}

impl Default for IntelligenceSettings {
    fn default() -> Self {
        Self {
            default_provider: default_intelligence_provider(),
        }
    }
}
