// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 由 QueryFile 候选清单构造的 DataFusion Parquet TableProvider。

mod groups;
mod pieces;

use std::{collections::HashMap, sync::Arc};

use arrow::{
    array::RecordBatch,
    datatypes::{DataType, Schema, SchemaRef, TimeUnit},
};
use async_trait::async_trait;
use datafusion::{
    catalog::{
        Session, TableProvider,
        memory::{DataSourceExec, MemorySourceConfig},
    },
    common::{DFSchema, ScalarValue},
    datasource::{
        listing::PartitionedFile,
        physical_plan::{
            FileScanConfigBuilder, ParquetSource,
            parquet::{CachedParquetFileReaderFactory, ParquetRowSelection},
        },
    },
    execution::{
        cache::{
            cache_manager::{CachedFileMetadataEntry, FileMetadataCache},
            default_cache::DefaultCache,
        },
        object_store::ObjectStoreUrl,
    },
    logical_expr::{Expr, TableProviderFilterPushDown, TableType},
    physical_expr::LexOrdering,
    physical_plan::{ExecutionPlan, Partitioning, empty::EmptyExec, union::UnionExec},
    prelude::{col, lit},
};
use domain::storage::{DatasetTypeId, QueryFile};
use groups::{Order, ScanFile, group, time_statistics};
use kernel::time::TimeRange;
use pieces::Cutter;

use crate::infra::storage::{
    arrow_schema::TS_COL,
    parquet::partition::{storage_ordering, storage_sort_column_names},
};

/// Footer bytes read with the first request to each file: the Thrift footer and the page index
/// that precedes it usually fit, so opening a file costs one request instead of two or three.
const FOOTER_READ_BYTES: usize = 512 * 1024;

/// Parsed Parquet footers (with page indexes) shared across queries, keyed by object key.
/// Primary Artifact objects are immutable, so an entry never goes stale.
pub type ParquetMetadataCache = Arc<FileMetadataCache>;

/// `None` when `max_bytes` is 0 (the cache is off).
pub fn parquet_metadata_cache(max_bytes: u64) -> Option<ParquetMetadataCache> {
    let limit = usize::try_from(max_bytes).unwrap_or(usize::MAX);

    (limit > 0).then(|| {
        Arc::new(
            DefaultCache::<object_store::path::Path, CachedFileMetadataEntry>::new(limit)
                .with_name("parquet_metadata"),
        ) as ParquetMetadataCache
    })
}

#[derive(Debug)]
pub struct PrunedParquetTable {
    schema: SchemaRef,
    /// Files that may hold rows outside `time_range`: read through the time filter.
    files: Vec<ScanFile>,
    /// Files whose every row lies inside `time_range`: the time filter holds for all their rows,
    /// so they are read without it.
    inside: Vec<ScanFile>,
    /// Files that follow each other in time may share a partition and keep its order: the storage
    /// order sorts by `_timestamp` first and every file tells DataFusion its time span.
    by_time: bool,
    object_store_url: ObjectStoreUrl,
    time_range: TimeRange,
    dataset_type: Option<DatasetTypeId>,
    buffered_batches: Vec<RecordBatch>,
    metadata_cache: Option<ParquetMetadataCache>,
}

impl PrunedParquetTable {
    pub fn new(
        schema: SchemaRef,
        query_files: &[QueryFile],
        object_store_url: ObjectStoreUrl,
        time_range: TimeRange,
        dataset_type: Option<DatasetTypeId>,
    ) -> Self {
        let timestamp = bounding_time_column(&schema);

        let (inside, files): (Vec<&QueryFile>, Vec<&QueryFile>) = query_files
            .iter()
            .partition(|file| timestamp.is_some() && within(time_range, file.time_range));

        let scanned = |files: Vec<&QueryFile>| -> Vec<ScanFile> {
            files
                .into_iter()
                .map(|file| {
                    let mut scanned =
                        PartitionedFile::new(file.object_key.clone(), file.size_bytes);

                    if let Some(index) = timestamp
                        && let Some(statistics) = time_statistics(&schema, index, file.time_range)
                    {
                        scanned = scanned.with_statistics(statistics);
                    }

                    ScanFile {
                        file: scanned,
                        time: file.time_range,
                        clustered: !file.clustering.is_empty(),
                        piece: false,
                    }
                })
                .collect()
        };

        let leading = storage_sort_column_names(&schema, dataset_type.as_ref())
            .into_iter()
            .find(|name| schema.index_of(name).is_ok());

        Self {
            files: scanned(files),
            inside: scanned(inside),
            by_time: timestamp.is_some() && leading == Some(TS_COL),
            schema,
            object_store_url,
            time_range,
            dataset_type,
            buffered_batches: Vec::new(),
            metadata_cache: None,
        }
    }

    pub fn with_buffered_batches(mut self, batches: Vec<RecordBatch>) -> Self {
        self.buffered_batches = batches;
        self
    }

    pub fn with_metadata_cache(mut self, cache: Option<ParquetMetadataCache>) -> Self {
        self.metadata_cache = cache;
        self
    }

    /// Rows to read from some files, by object key: a file with a selection is read only where
    /// those rows lie. The filters still run on every row read, so a selection only has to hold
    /// every row the query can return.
    pub fn with_row_selections(
        mut self,
        selections: &HashMap<String, ParquetRowSelection>,
    ) -> Self {
        if selections.is_empty() {
            return self;
        }

        let select = |files: Vec<ScanFile>| -> Vec<ScanFile> {
            files
                .into_iter()
                .map(|mut scanned| {
                    if let Some(selection) =
                        selections.get(scanned.file.object_meta.location.as_ref())
                    {
                        scanned.file = scanned.file.with_extension(selection.clone());
                    }

                    scanned
                })
                .collect()
        };

        self.files = select(std::mem::take(&mut self.files));
        self.inside = select(std::mem::take(&mut self.inside));
        self
    }

    /// QueryRequest 的绝对时间窗在物理扫描层强制执行。使用带 UTC 类型的 literal，避免
    /// Timestamp(µs, UTC) 与 Int64 比较时的规划错误。
    fn time_filter(&self) -> Expr {
        let timezone = Some("UTC".into());

        col(TS_COL)
            .gt_eq(lit(ScalarValue::TimestampMicrosecond(
                Some(self.time_range.start.0),
                timezone.clone(),
            )))
            .and(col(TS_COL).lt(lit(ScalarValue::TimestampMicrosecond(
                Some(self.time_range.end.0),
                timezone,
            ))))
    }

    /// Reads `files` in at most `width` execution partitions, so at most `width` files at once,
    /// with `predicate` pushed into the Parquet reader.
    ///
    /// The statement's own filters are not given here: they stay in a filter above the scan
    /// (pushdown is inexact) and DataFusion's filter pushdown moves them into the reader. Given here
    /// as well, the reader would evaluate each of them twice, and a `LIKE` evaluated twice decodes
    /// its column twice, the second time for the few rows the first left.
    fn file_scan(
        &self,
        state: &dyn Session,
        files: &[ScanFile],
        width: usize,
        predicate: Option<Expr>,
        projection: Option<&Vec<usize>>,
        limit: Option<usize>,
    ) -> datafusion::common::Result<Arc<dyn ExecutionPlan>> {
        let mut source = ParquetSource::new(self.schema.clone())
            .with_pushdown_filters(true)
            .with_bloom_filter_on_read(true)
            .with_metadata_size_hint(FOOTER_READ_BYTES);

        if let Some(filter) = predicate {
            let df_schema = DFSchema::try_from(self.schema.clone())?;
            source = source.with_predicate(state.create_physical_expr(filter, &df_schema)?);
        }

        if let Some(cache) = &self.metadata_cache {
            // The org-scoped store this query registered; the cache only skips footer reads.
            let store = state.runtime_env().object_store(&self.object_store_url)?;

            source = source.with_parquet_file_reader_factory(Arc::new(
                CachedParquetFileReaderFactory::new(store, cache.clone()),
            ));
        }

        let (file_groups, order) = group(files, width, self.by_time);

        // Writer / compactor 均按这组键降序写每个物理文件。分区声明有序后，DataFusion 可以对
        // ORDER BY + LIMIT 使用 SortPreservingMerge / Top-K，而不是先读完所有文件。
        let ordering = match order {
            Order::PerFile => self.physical_ordering(),
            Order::NewestFirst => self.time_ordering(),
            Order::Unordered => Vec::new(),
        };

        // Pieces were cut for exactly these partitions. Declared, they stay as they are: DataFusion
        // would otherwise split the scan once more, into equal byte ranges that ignore the pieces.
        let partitioning = files
            .iter()
            .any(|scanned| scanned.piece)
            .then(|| Partitioning::UnknownPartitioning(file_groups.len()));

        let config = FileScanConfigBuilder::new(self.object_store_url.clone(), Arc::new(source))
            .with_projection_indices(projection.cloned())?
            .with_limit(limit)
            .with_file_groups(file_groups)
            .with_output_ordering(ordering)
            .with_output_partitioning(partitioning)
            .build();

        Ok(Arc::new(DataSourceExec::new(Arc::new(config))))
    }

    /// The order the writer and the compactor give every file.
    fn physical_ordering(&self) -> Vec<LexOrdering> {
        storage_ordering(self.schema.as_ref(), self.dataset_type.as_ref())
            .into_iter()
            .collect()
    }

    /// The storage order's first key, `_timestamp` descending: what files read one after another
    /// keep when each ends no later than the one before starts.
    fn time_ordering(&self) -> Vec<LexOrdering> {
        self.physical_ordering()
            .into_iter()
            .filter_map(|ordering| LexOrdering::new([ordering.first().clone()]))
            .collect()
    }
}

#[async_trait]
impl TableProvider for PrunedParquetTable {
    fn schema(&self) -> SchemaRef {
        self.schema.clone()
    }

    fn table_type(&self) -> TableType {
        TableType::Base
    }

    fn supports_filters_pushdown(
        &self,
        filters: &[&Expr],
    ) -> datafusion::common::Result<Vec<TableProviderFilterPushDown>> {
        Ok(vec![TableProviderFilterPushDown::Inexact; filters.len()])
    }

    /// The statement's `filters` reach the Parquet reader through DataFusion's filter pushdown, not
    /// through this scan (see [`Self::file_scan`]); the scan adds only the request's time window,
    /// to the files the window cuts.
    async fn scan(
        &self,
        state: &dyn Session,
        projection: Option<&Vec<usize>>,
        _filters: &[Expr],
        limit: Option<usize>,
    ) -> datafusion::common::Result<Arc<dyn ExecutionPlan>> {
        if self.files.is_empty() && self.inside.is_empty() && self.buffered_batches.is_empty() {
            let schema = projection
                .map(|indices| self.schema.project(indices).map(Arc::new))
                .transpose()?
                .unwrap_or_else(|| self.schema.clone());

            return Ok(Arc::new(EmptyExec::new(schema)));
        }

        // The session's parallelism caps how many files the query reads at once; the two file
        // scans share it.
        let (inside_width, files_width) = shares(
            state.config().target_partitions(),
            self.inside.len(),
            self.files.len(),
        );

        // A scan of fewer files than its partitions reads them in pieces, so that every partition
        // reads about as many rows; the files the window cuts count only the rows it may hold.
        let cutter = Cutter::new(state, &self.object_store_url, self.metadata_cache.as_ref());

        let (inside, files) = futures::join!(
            pieces::read(cutter.as_ref(), &self.inside, inside_width, None),
            pieces::read(
                cutter.as_ref(),
                &self.files,
                files_width,
                Some(self.time_range)
            ),
        );

        let mut plans: Vec<Arc<dyn ExecutionPlan>> = Vec::with_capacity(3);

        if !inside.is_empty() {
            plans.push(self.file_scan(state, &inside, inside_width, None, projection, limit)?);
        }

        if !files.is_empty() {
            plans.push(self.file_scan(
                state,
                &files,
                files_width,
                Some(self.time_filter()),
                projection,
                limit,
            )?);
        }

        if !self.buffered_batches.is_empty() {
            plans.push(MemorySourceConfig::try_new_exec(
                std::slice::from_ref(&self.buffered_batches),
                self.schema.clone(),
                projection.cloned(),
            )?);
        }

        UnionExec::try_new(plans)
    }
}

/// The index of `_timestamp` when a file's recorded span bounds all its rows: the column is a
/// microsecond timestamp no row leaves NULL. A NULL fails the time filter but is not in the span.
fn bounding_time_column(schema: &Schema) -> Option<usize> {
    let index = schema.index_of(TS_COL).ok()?;
    let field = schema.field(index);

    let micros = matches!(
        field.data_type(),
        DataType::Timestamp(TimeUnit::Microsecond, _)
    );

    (micros && !field.is_nullable()).then_some(index)
}

/// The file's rows, whose `_timestamp` spans `file` (both ends included), all lie in `window`.
fn within(window: TimeRange, file: TimeRange) -> bool {
    window.start.0 <= file.start.0 && file.end.0 < window.end.0
}

/// `width` partitions shared by two scans in proportion to their files, at least one each. A
/// scan without files leaves the other all of them.
fn shares(width: usize, first: usize, second: usize) -> (usize, usize) {
    if first == 0 || second == 0 {
        return (width, width);
    }

    let first_share = (width.saturating_mul(first) / (first + second)).clamp(1, width.max(2) - 1);

    (first_share, width.saturating_sub(first_share).max(1))
}

#[cfg(test)]
mod tests {
    use arrow::{
        array::{Int64Array, TimestampMicrosecondArray},
        datatypes::{DataType, Field, Schema, TimeUnit},
    };
    use datafusion::{
        common::TableReference,
        physical_plan::displayable,
        prelude::{SessionConfig, SessionContext},
    };
    use domain::{storage::primary_dataset_type, stream::StreamType};
    use kernel::{ids::Id, time::TimestampMicros};
    use object_store::{ObjectStoreExt, PutPayload, memory::InMemory, path::Path};
    use parquet::{
        arrow::{
            ArrowWriter,
            arrow_reader::{RowSelection, RowSelector},
        },
        file::properties::WriterProperties,
    };

    use super::*;
    use crate::infra::storage::filters::BLOCK_ROWS;

    const STORE: &str = "molesignal://grouping-test";

    fn logs_schema() -> SchemaRef {
        Arc::new(Schema::new(vec![
            Field::new(
                TS_COL,
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new("value", DataType::Int64, false),
        ]))
    }

    /// One Parquet file per list of row times, each in storage order with `value` equal to its
    /// time, in an in-memory store that a session with `parallelism` partitions reads.
    async fn stored(rows: &[Vec<i64>], parallelism: usize) -> (SessionContext, Vec<QueryFile>) {
        stored_in(rows, parallelism, None).await
    }

    /// As [`stored`]; with `group_rows`, the files have row groups of that many rows and pages of
    /// a block, and the session splits scans of any size.
    async fn stored_in(
        rows: &[Vec<i64>],
        parallelism: usize,
        group_rows: Option<usize>,
    ) -> (SessionContext, Vec<QueryFile>) {
        let schema = logs_schema();
        let store = InMemory::new();
        let mut files = Vec::new();

        for (index, times) in rows.iter().enumerate() {
            let mut times = times.clone();
            times.sort_unstable_by(|left, right| right.cmp(left));

            let batch = RecordBatch::try_new(
                schema.clone(),
                vec![
                    Arc::new(TimestampMicrosecondArray::from(times.clone()).with_timezone("UTC")),
                    Arc::new(Int64Array::from(times.clone())),
                ],
            )
            .unwrap();

            let properties = group_rows.map(|group_rows| {
                WriterProperties::builder()
                    .set_max_row_group_row_count(Some(group_rows))
                    .set_data_page_row_count_limit(BLOCK_ROWS)
                    .set_write_batch_size(BLOCK_ROWS)
                    .build()
            });

            let mut bytes = Vec::new();
            let mut writer = ArrowWriter::try_new(&mut bytes, schema.clone(), properties).unwrap();
            writer.write(&batch).unwrap();
            writer.close().unwrap();

            let key = format!("f{index}.parquet");
            let size = bytes.len() as u64;

            store
                .put(&Path::from(key.as_str()), PutPayload::from(bytes))
                .await
                .unwrap();

            files.push(QueryFile {
                id: Id::from_string(&key),
                org_id: Id::from_string("org"),
                stream: "logs".into(),
                stream_type: StreamType::LOGS,
                dataset_type: primary_dataset_type(StreamType::LOGS).unwrap(),
                object_key: key,
                checksum: None,
                etag: None,
                time_range: TimeRange::new(
                    TimestampMicros(times[times.len() - 1]),
                    TimestampMicros(times[0]),
                ),
                rows: times.len() as u64,
                size_bytes: size,
                min_values: Default::default(),
                max_values: Default::default(),
                value_sets: Default::default(),
                address_ranges: Default::default(),
                null_counts: Default::default(),
                indexed_at_micros: None,
                shard: None,
                clustering: Default::default(),
            });
        }

        let mut config = SessionConfig::new().with_target_partitions(parallelism);

        if group_rows.is_some() {
            config = config.with_repartition_file_min_size(0);
        }

        let context = SessionContext::new_with_config(config);

        context.runtime_env().register_object_store(
            ObjectStoreUrl::parse(STORE).unwrap().as_ref(),
            Arc::new(store),
        );

        (context, files)
    }

    /// The files read through the window from `start` to `end`.
    fn table(files: &[QueryFile], start: i64, end: i64) -> PrunedParquetTable {
        PrunedParquetTable::new(
            logs_schema(),
            files,
            ObjectStoreUrl::parse(STORE).unwrap(),
            TimeRange::new(TimestampMicros(start), TimestampMicros(end)),
            None,
        )
    }

    fn register(context: &SessionContext, files: &[QueryFile], start: i64, end: i64) {
        register_table(context, table(files, start, end));
    }

    fn register_table(context: &SessionContext, table: PrunedParquetTable) {
        context
            .register_table(TableReference::bare("logs"), Arc::new(table))
            .unwrap();
    }

    async fn physical_plan(context: &SessionContext, sql: &str) -> String {
        let plan = context
            .sql(sql)
            .await
            .unwrap()
            .create_physical_plan()
            .await
            .unwrap();

        displayable(plan.as_ref()).indent(true).to_string()
    }

    async fn first_column(context: &SessionContext, sql: &str) -> Vec<i64> {
        let batches = context.sql(sql).await.unwrap().collect().await.unwrap();

        batches
            .iter()
            .flat_map(|batch| {
                batch
                    .column(0)
                    .as_any()
                    .downcast_ref::<Int64Array>()
                    .unwrap()
                    .values()
                    .to_vec()
            })
            .collect()
    }

    /// Ten rows a hour, each hour a file, `hours` of them from the hour 0.
    fn hourly(hours: i64) -> Vec<Vec<i64>> {
        (0..hours)
            .map(|hour| (hour * 100..hour * 100 + 10).collect())
            .collect()
    }

    #[tokio::test]
    async fn files_that_follow_each_other_are_read_a_few_at_a_time_in_time_order() {
        let (context, files) = stored(&hourly(12), 3).await;
        register(&context, &files, 0, 10_000);

        let latest = "SELECT value FROM logs ORDER BY _timestamp DESC LIMIT 5";
        let plan = physical_plan(&context, latest).await;

        assert!(plan.contains("file_groups={3 groups"), "{plan}");
        assert!(
            !plan.contains("SortExec"),
            "the groups keep the time order:\n{plan}"
        );
        assert_eq!(
            first_column(&context, latest).await,
            [1109, 1108, 1107, 1106, 1105]
        );
        assert_eq!(
            first_column(&context, "SELECT COUNT(*) FROM logs").await,
            [120]
        );
    }

    #[tokio::test]
    async fn files_cut_by_the_window_share_the_partitions_with_the_rest() {
        // Hours 2 and 7 hold rows outside the window, hours 3 to 6 lie inside it.
        let (context, files) = stored(&hourly(8)[2..], 3).await;
        register(&context, &files, 205, 705);

        let latest = "SELECT value FROM logs ORDER BY _timestamp DESC LIMIT 3";
        let plan = physical_plan(&context, latest).await;

        assert!(plan.contains("file_groups={2 groups"), "{plan}");
        assert!(plan.contains("file_groups={1 group:"), "{plan}");
        assert!(!plan.contains("SortExec"), "{plan}");
        assert_eq!(first_column(&context, latest).await, [704, 703, 702]);
        assert_eq!(
            first_column(&context, "SELECT COUNT(*) FROM logs").await,
            [50]
        );
    }

    /// The predicate of each Parquet scan of `plan`.
    fn scan_predicates(plan: &str) -> Vec<&str> {
        plan.lines()
            .filter_map(|line| line.split(" predicate=").nth(1))
            .map(|rest| {
                let end = [", pruning_predicate=", ", required_guarantees="]
                    .iter()
                    .filter_map(|key| rest.find(key))
                    .min()
                    .unwrap_or(rest.len());

                &rest[..end]
            })
            .collect()
    }

    #[tokio::test]
    async fn the_reader_gets_each_filter_once() {
        // Hours 2 and 7 hold rows outside the window, hours 3 to 6 lie inside it.
        let (context, files) = stored(&hourly(8)[2..], 3).await;
        register(&context, &files, 205, 705);

        let even = "SELECT COUNT(*) FROM logs WHERE value % 2 = 0";
        let plan = physical_plan(&context, even).await;
        let predicates = scan_predicates(&plan);

        // Both scans get the statement's filter once; the files the window cuts get the window.
        assert_eq!(predicates.len(), 2, "{plan}");

        for predicate in &predicates {
            assert_eq!(predicate.matches("value@1 % 2 = 0").count(), 1, "{plan}");
        }

        assert_eq!(
            predicates
                .iter()
                .filter(|predicate| predicate.contains("_timestamp@0 >= 205"))
                .count(),
            1,
            "{plan}"
        );
        assert_eq!(first_column(&context, even).await, [25]);
    }

    #[tokio::test]
    async fn overlapping_files_are_read_a_few_at_a_time_and_sorted() {
        // Six files over the same span: none can follow another.
        let rows = (0..6)
            .map(|file| (0..10).map(|row| row * 6 + file).collect())
            .collect::<Vec<Vec<i64>>>();

        let (context, files) = stored(&rows, 2).await;
        register(&context, &files, 0, 10_000);

        let latest = "SELECT value FROM logs ORDER BY _timestamp DESC LIMIT 3";
        let plan = physical_plan(&context, latest).await;

        assert!(plan.contains("file_groups={2 groups"), "{plan}");
        assert!(plan.contains("SortExec"), "{plan}");
        assert_eq!(first_column(&context, latest).await, [59, 58, 57]);
        assert_eq!(
            first_column(&context, "SELECT COUNT(*) FROM logs").await,
            [60]
        );
    }

    /// One file of 61,440 rows at the times `0..61_440`, in row groups of `group_rows` rows, read
    /// by three partitions.
    async fn one_file(group_rows: usize) -> (SessionContext, Vec<QueryFile>) {
        stored_in(&[(0..61_440).collect()], 3, Some(group_rows)).await
    }

    fn cached(table: PrunedParquetTable) -> PrunedParquetTable {
        table.with_metadata_cache(parquet_metadata_cache(64 * 1024 * 1024))
    }

    const THREE_PIECES: &str = "file_groups={3 groups: [[f0.parquet], [f0.parquet], [f0.parquet]]}";
    const TWO_PIECES: &str = "file_groups={2 groups: [[f0.parquet], [f0.parquet]]}";

    #[tokio::test]
    async fn a_file_of_one_row_group_is_read_by_every_partition() {
        let (context, files) = one_file(1 << 20).await;
        register_table(&context, cached(table(&files, 0, 1_000_000)));

        let latest = "SELECT value FROM logs ORDER BY _timestamp DESC LIMIT 3";
        let plan = physical_plan(&context, latest).await;

        assert!(plan.contains(THREE_PIECES), "{plan}");
        assert_eq!(
            first_column(&context, latest).await,
            [61_439, 61_438, 61_437]
        );
        assert_eq!(
            first_column(&context, "SELECT COUNT(*) FROM logs").await,
            [61_440]
        );
        assert_eq!(
            first_column(&context, "SELECT COUNT(*) FROM logs WHERE value % 2 = 0").await,
            [30_720]
        );
    }

    #[tokio::test]
    async fn pieces_share_out_the_rows_of_unequal_row_groups() {
        // Row groups of 16, 16, 16 and 12 blocks: of three equal byte ranges, the first would hold
        // the start of two of them.
        let (context, files) = one_file(16 * BLOCK_ROWS).await;
        register_table(&context, cached(table(&files, 0, 1_000_000)));

        let count = "SELECT COUNT(*) FROM logs";
        let plan = physical_plan(&context, count).await;

        assert!(plan.contains(THREE_PIECES), "{plan}");
        assert_eq!(first_column(&context, count).await, [61_440]);
    }

    #[tokio::test]
    async fn pieces_of_a_file_the_window_cuts_hold_only_its_pages() {
        // The times 10,000..50,000 lie in the rows 11,440..51,440, in the pages from row 11,264 to
        // 52,224: enough rows for two pieces, not three.
        let (context, files) = one_file(16 * BLOCK_ROWS).await;
        register_table(&context, cached(table(&files, 10_000, 50_000)));

        let count = "SELECT COUNT(*) FROM logs";
        let plan = physical_plan(&context, count).await;

        assert!(plan.contains(TWO_PIECES), "{plan}");
        assert_eq!(first_column(&context, count).await, [40_000]);
    }

    #[tokio::test]
    async fn pieces_read_only_the_rows_an_index_chose() {
        let (context, files) = one_file(16 * BLOCK_ROWS).await;

        // The index chose the first 20,480 rows and the last.
        let chosen = RowSelection::from(vec![
            RowSelector::select(20_480),
            RowSelector::skip(20_480),
            RowSelector::select(20_480),
        ]);
        let selections =
            HashMap::from([("f0.parquet".to_owned(), ParquetRowSelection::new(chosen))]);

        register_table(
            &context,
            cached(table(&files, 0, 1_000_000)).with_row_selections(&selections),
        );

        let count = "SELECT COUNT(*) FROM logs";
        let plan = physical_plan(&context, count).await;

        assert!(plan.contains(TWO_PIECES), "{plan}");
        assert_eq!(first_column(&context, count).await, [40_960]);
    }

    #[tokio::test]
    async fn the_spare_partitions_go_to_the_largest_file() {
        let rows = [(0..49_152).collect(), (100_000..116_384).collect()];
        let (context, files) = stored_in(&rows, 3, Some(16 * BLOCK_ROWS)).await;
        register_table(&context, cached(table(&files, 0, 1_000_000)));

        let latest = "SELECT value FROM logs ORDER BY _timestamp DESC LIMIT 2";
        let plan = physical_plan(&context, latest).await;

        assert!(
            plan.contains("file_groups={3 groups: [[f0.parquet], [f0.parquet], [f1.parquet]]}"),
            "{plan}"
        );
        assert_eq!(first_column(&context, latest).await, [116_383, 116_382]);
        assert_eq!(
            first_column(&context, "SELECT COUNT(*) FROM logs").await,
            [65_536]
        );
    }

    #[tokio::test]
    async fn buffered_batches_participate_in_normal_filter_and_projection() {
        let schema = Arc::new(Schema::new(vec![
            Field::new(
                "_timestamp",
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new("value", DataType::Int64, false),
        ]));
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(TimestampMicrosecondArray::from(vec![10, 20, 30]).with_timezone("UTC")),
                Arc::new(Int64Array::from(vec![1, 2, 3])),
            ],
        )
        .unwrap();
        let table = PrunedParquetTable::new(
            schema,
            &[],
            ObjectStoreUrl::parse("molesignal://buffer-test").unwrap(),
            TimeRange::new(TimestampMicros(0), TimestampMicros(100)),
            None,
        )
        .with_buffered_batches(vec![batch]);
        let context = SessionContext::new();
        context
            .register_table(TableReference::bare("metrics"), Arc::new(table))
            .unwrap();

        let batches = context
            .sql("SELECT value FROM metrics WHERE value >= 2 ORDER BY value")
            .await
            .unwrap()
            .collect()
            .await
            .unwrap();
        let values = batches
            .iter()
            .flat_map(|batch| {
                batch
                    .column(0)
                    .as_any()
                    .downcast_ref::<Int64Array>()
                    .unwrap()
                    .values()
                    .iter()
                    .copied()
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(values, vec![2, 3]);
    }
}
