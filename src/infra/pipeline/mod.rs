// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Pipeline runtime。
//!
//! 该 crate 子模块提供：
//! - [`extend_table::ExtendTable`]：内存 KV 表 + 监听 extend stream 写入事件
//!   重建；提供 `lookup(table, key)` 给 VRL runtime。
//! - [`extend_table::repository`]：`extend_kv` 表 CRUD + 表级 list（rebuild 用）。
//! - [`scheduled::ScheduledPipelineRunner`]：cron 解析 + alert_manager / scheduler
//!   role tick 内调；每 run 走 SQL 查询 + 函数链 + 写目标 stream（标准 ingest）。
//! - [`scheduled::repository`]：`scheduled_pipelines` 表 CRUD。

pub mod exec;
pub mod extend_table;
pub mod scheduled;

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
