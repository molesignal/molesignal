// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 读一个选择器的样本：持有其指标的流里，时间窗内的样本文件同时读几个，只读该指标所在的页；
//! 再在内存里按指标名与 matchers 过滤、按 label 分组成 series。

use futures::{StreamExt, TryStreamExt, stream};

use super::*;
use crate::{
    domain::metrics::METRIC_NAME_FIELD, infra::query::scan_limit::ensure_within_scan_limit,
    shared::time::TimestampMicros,
};

/// 同时读的样本文件数。对象存储上每个文件至少一次往返，一个接一个读时延迟随文件数相加。
const CONCURRENT_FILE_READS: usize = 8;

impl PromQLEngine {
    /// 拉取整个查询时间范围，以及 range 函数计算所需的前置窗口。
    pub(super) async fn load_matrix_for_query_range(
        &self,
        vs: &VectorSelector,
        matrix_range: Duration,
        req: &QueryRequest,
    ) -> Result<Vec<Series>> {
        let span_us = req
            .time_range
            .end
            .0
            .saturating_sub(req.time_range.start.0)
            .max(1);

        let matrix_us = (matrix_range.as_micros() as i64).max(1);
        let total_us = span_us.saturating_add(matrix_us).max(1);

        self.load_matrix(
            vs,
            Duration::from_micros(total_us as u64),
            req.time_range.end.0,
            req,
        )
        .await
    }

    /// 从 `(at_us - range, at_us]` 拉一个 matrix（每个 series 的样本序列）。
    /// 实现：按 QueryFile 裁剪时间窗，同时读几个文件；Parquet 只投影 timestamp/value/label 列，
    /// 按 row group 统计与页索引跳过别的指标和窗口外的部分，再在内存按指标名与 matchers 过滤、
    /// 按 labels 分组。读到的行数超过 `max_scan_rows` 时停下并报错。
    pub(super) async fn load_matrix(
        &self,
        vs: &VectorSelector,
        range: Duration,
        at_us: i64,
        req: &QueryRequest,
    ) -> Result<Vec<Series>> {
        let filter = MetricFilter::of(vs);

        // 选择器上的 `@` / `offset` 修饰：先把求值时刻平移到生效时刻。
        let at_us = effective_eval_time(vs, at_us, req);
        let range_us = range.as_micros() as i64;
        let start_us = at_us - range_us;
        let window = TimeRange::new(TimestampMicros(start_us), TimestampMicros(at_us));
        let sources = self.metric_sources(&req.org_id, &filter, window).await?;

        // 样本文件只能从请求组织自己的对象前缀读取。
        let store = scoped_to_org(self.object_store.clone(), &req.org_id);
        let reader = ParquetReader::new(store.clone());
        let mut batches = Vec::new();
        let mut rows = 0_u64;

        for source in &sources {
            let storage = self
                .metric_files(&req.org_id, &source.stream, window)
                .await?;

            let projection = source
                .sample_columns
                .as_ref()
                .map(|columns| columns.iter().map(String::as_str).collect::<Vec<_>>());

            // 流里放拥有所有权的 key：future 借用 `storage.files` 的元素会让 `Send` 推断失败。
            let files = storage
                .files
                .iter()
                .map(|fm| (fm.object_key.clone(), fm.size_bytes))
                .collect::<Vec<_>>();

            let (reader, projection, name) = (&reader, projection.as_deref(), filter.name());

            let reads = files.into_iter().map(|(object_key, size_bytes)| {
                let store = store.clone();

                async move {
                    // 时间窗口下推：边界文件只解码与窗口相交的 row group 与页（裁剪的窗口左闭右开，
                    // 样本窗口含右端）；QueryFile 里的权威 size 避免每文件再做一次 HEAD。
                    let mut options = ReadOptions::new()
                        .with_time_range(start_us, at_us.saturating_add(1))
                        .with_known_size(size_bytes);

                    if let Some(name) = name {
                        options = options.with_equals(METRIC_NAME_FIELD, name);
                    }

                    if let Some(columns) = projection {
                        options = options.with_columns(columns);
                    }

                    reader.read_from_store(store, &object_key, options).await
                }
            });

            // 按文件的顺序收齐：同一 series 同一时刻的重复样本仍以后面的文件为准。
            let mut read = stream::iter(reads).buffered(CONCURRENT_FILE_READS);

            while let Some(file_batches) = read.try_next().await? {
                rows += file_batches
                    .iter()
                    .map(|batch| batch.num_rows() as u64)
                    .sum::<u64>();

                ensure_within_scan_limit(rows, self.max_scan_rows)?;
                batches.extend(file_batches);
            }

            batches.extend(storage.buffered_batches);
        }

        if batches.is_empty() {
            return Ok(Vec::new());
        }

        batches_to_series(
            &batches,
            &vs.matchers,
            &filter,
            start_us,
            at_us,
            MAX_MATRIX_SAMPLES,
        )
    }
}
