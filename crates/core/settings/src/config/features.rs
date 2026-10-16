// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 可选功能与集成：`[functions]`（VRL/JS/LLM 运行时）、`[mmdb]`（GeoIP）、
//! `[scheduled_reports]`（headless 渲染）、`[log_inspection]`（定时日志巡检的额度）。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FunctionsSettings {
    /// LLM 评估节点开关。默认 false：`language = llm` 的 pipeline 步骤会被拒
    /// （`llm eval runtime disabled`）。开启后，每条命中事件都会触发一次模型调用 ——
    /// **成本与延迟随 intake 量线性放大**，请仅在低吞吐流上启用，并先配好 org 的 AI provider。
    #[serde(default)]
    pub llm_eval_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MmdbSettings {
    /// 本地 GeoLite2 City mmdb 文件路径。库随版本库（`assets/mmdb`）进入容器镜像和二进制包，
    /// 运行期不下载。
    #[serde(default = "default_mmdb_path")]
    pub db_path: String,
}

fn default_mmdb_path() -> String {
    "/usr/share/molesignal/mmdb/GeoLite2-City.mmdb".into()
}

impl Default for MmdbSettings {
    fn default() -> Self {
        Self {
            db_path: default_mmdb_path(),
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

/// `[log_inspection]`：定时日志巡检。每次巡检都要调用 AI 模型，这一段限定它能用掉多少。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogInspectionSettings {
    /// 同一条巡检两次执行之间的最短间隔（秒）；小于 60 时按 60 处理。
    #[serde(default = "default_inspection_min_interval_secs")]
    pub min_interval_secs: u64,
    /// 每个组织最多保存多少条定时巡检；0 表示不允许创建。
    #[serde(default = "default_inspection_max_per_org")]
    pub max_per_org: u32,
    /// 单节点同时执行的巡检数；0 按 1 处理。
    #[serde(default = "default_inspection_concurrent_runs")]
    pub concurrent_runs: u32,
    /// 单次巡检的总超时（秒），含全部模型调用与查询；小于 30 按 30 处理，大于 480 时收敛为
    /// 480，保证超时的执行在运行租约到期前结束。
    #[serde(default = "default_inspection_run_timeout_secs")]
    pub run_timeout_secs: u64,
    /// 巡检报告保留天数；0 按 1 处理。
    #[serde(default = "default_inspection_report_retention_days")]
    pub report_retention_days: u32,
}

fn default_inspection_min_interval_secs() -> u64 {
    3600
}

fn default_inspection_max_per_org() -> u32 {
    20
}

fn default_inspection_concurrent_runs() -> u32 {
    2
}

fn default_inspection_run_timeout_secs() -> u64 {
    300
}

fn default_inspection_report_retention_days() -> u32 {
    30
}

impl Default for LogInspectionSettings {
    fn default() -> Self {
        Self {
            min_interval_secs: default_inspection_min_interval_secs(),
            max_per_org: default_inspection_max_per_org(),
            concurrent_runs: default_inspection_concurrent_runs(),
            run_timeout_secs: default_inspection_run_timeout_secs(),
            report_retention_days: default_inspection_report_retention_days(),
        }
    }
}
