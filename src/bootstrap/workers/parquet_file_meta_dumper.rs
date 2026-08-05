// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! ParquetFileMeta dump worker（spec `storage/ParquetFileMeta Dump Spillover`）—— 调度层。
//!
//! 实际 SQL + object_store IO 在 `crate::infra::storage::parquet_file_meta_dump`；
//! 本模块只负责按 `interval_secs` tick 起 [`ParquetFileMetaDumpService::run_tick`]，并 log。
//!
//! 与 compactor 同 role：bootstrap 在 compactor role 决定 spawn 时一并调本模块的
//! `spawn`。`enabled = false` 时直接退出，不进 tick 循环。

use std::sync::Arc;

use tokio::task::JoinHandle;

use crate::infra::storage::parquet_file_meta_dump::ParquetFileMetaDumpService;

/// 每 tick 最多清理多少个被 mark_deleted 的冷 dump 对象（bounded，跨 tick 逐步排空）。
const OBJECT_DELETE_SWEEP_LIMIT: u32 = 500;

pub fn spawn(service: Arc<ParquetFileMetaDumpService>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let s = service.settings();
        if !s.enabled || s.max_partitions_per_tick == 0 {
            tracing::info!("parquet_file_meta_dump worker disabled");
            return;
        }
        tracing::info!(
            cold_after_days = s.cold_after_days,
            interval_secs = s.interval_secs,
            max_partitions_per_tick = s.max_partitions_per_tick,
            "parquet_file_meta_dump worker started"
        );
        let interval = service.interval();
        loop {
            match service.run_tick().await {
                Ok(stats) if stats.partitions_processed > 0 || stats.partitions_skipped > 0 => {
                    tracing::info!(
                        partitions = stats.partitions_processed,
                        skipped = stats.partitions_skipped,
                        rows = stats.rows_dumped,
                        "parquet_file_meta_dump tick complete"
                    );
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(error = %e, "parquet_file_meta_dump tick failed");
                }
            }
            // 对象删除 sweep：清理被 mark_deleted 但 parquet 仍残留的冷 dump 对象，
            // 防 object_store 空间泄漏（change `parquet-file-meta-dump-columnar` task 8.x）。
            match service
                .run_object_delete_sweep(OBJECT_DELETE_SWEEP_LIMIT)
                .await
            {
                Ok(n) if n > 0 => {
                    tracing::info!(
                        purged = n,
                        "parquet_file_meta_dump object-delete sweep purged objects"
                    );
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(error = %e, "parquet_file_meta_dump object-delete sweep failed");
                }
            }
            tokio::time::sleep(interval).await;
        }
    })
}
