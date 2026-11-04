// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Pipeline runtime。
//!
//! 该 crate 子模块提供：
//! - [`extend_table::ExtendTable`]：扩展表的内存副本，用到时从 `extend_kv` 读进来，给 VRL 的
//!   `lookup(table, key)` 查。
//! - [`extend_table::repository`]：`extend_kv` 表 CRUD + 表级 list。
//! - [`lookups::PlatformLookups`]：本节点 VRL 的 `lookup` 与 `geoip_lookup` 读的数据。
//! - [`scheduled::ScheduledPipelineRunner`]：cron 解析 + alert / scheduler
//!   role tick 内调；每 run 走 SQL 查询 + 函数链 + 写目标 stream（标准 intake）。
//! - [`scheduled::repository`]：`scheduled_pipelines` 表 CRUD。
//! - [`realtime`]：写入路径读的实时流水线，按组织缓存。
//! - [`alert`]：开事件前读的告警流水线，按组织缓存，并记下各自处理的结果。

pub mod alert;
pub mod exec;
pub mod extend_table;
pub mod lookups;
pub mod realtime;
pub mod scheduled;

use std::sync::atomic::{AtomicU64, Ordering};

pub use extend_table::{
    ExtendTable,
    repository::{
        ExtendKvRepository, ExtendRow, ExtendTableDefinition, ExtendTableSummary, ExtendValueField,
        PgExtendKvRepository,
    },
};
pub use scheduled::{
    PipelineExecutor, ScheduledPipelineRunner,
    repository::{PgScheduledPipelineRepository, ScheduledPipeline, ScheduledPipelineRepository},
};

/// 本节点改流水线配置的次数。缓存着配置的地方记下读取时的值，值变了就重读。
static CHANGES: AtomicU64 = AtomicU64::new(0);

/// 本节点建、改或删了一条流水线：缓存着配置的地方下一次重读。别的节点只能等缓存过期。
pub fn note_change() {
    CHANGES.fetch_add(1, Ordering::Relaxed);
}

/// 本节点到目前为止改流水线配置的次数，见 [`note_change`]。
pub fn changes() -> u64 {
    CHANGES.load(Ordering::Relaxed)
}
