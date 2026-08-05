## ADDED Requirements

### Requirement: Multi-Backend Object Store

The system SHALL build an `object_store::ObjectStore` from `[object_store]` settings supporting `backend = "local" | "s3" | "azure" | "gcs"`, with credentials, region, bucket, and endpoint read from the same config table.

#### Scenario: Local backend uses prefixed directory
- **WHEN** `backend = "local"` and `root = "./data/objects"`
- **THEN** the constructed store writes under that directory and creates it if missing

#### Scenario: S3 backend uses configured credentials
- **WHEN** `backend = "s3"` and `access_key`, `secret_key`, `region`, `bucket` are populated
- **THEN** the store routes requests to AWS S3 (or the `endpoint` override for MinIO-compatible stores)

#### Scenario: Unknown backend
- **WHEN** `backend` is not one of the four supported values
- **THEN** server startup fails with `Error::Invalid("unsupported object_store backend: <name>")`

### Requirement: Parquet Writer

The system SHALL serialize an `Arrow RecordBatch` (built from a flushed in-memory buffer) to a parquet stream with snappy compression and upload it to the object store under `{org}/{stream}/{YYYY-MM-DD}/{ksuid}.parquet`, recording a `ParquetFileMeta` row before returning success.

#### Scenario: Successful write inserts ParquetFileMeta
- **WHEN** the writer finishes uploading
- **THEN** a `ParquetFileMeta` row is inserted with `object_key`, `time_range = min..=max(_timestamp)`, `rows`, `size_bytes`, `min_values`/`max_values` for indexed fields, and `deleted = false`

#### Scenario: ParquetFileMeta insert failure deletes orphan object
- **WHEN** the parquet upload succeeds but the `ParquetFileMetaRepository::insert` call fails
- **THEN** the writer attempts to delete the just-uploaded object and bubbles the original error

### Requirement: Parquet Reader

The system SHALL stream parquet files from the object store as Arrow `RecordBatch`es with predicate and projection pushdown, exposed via a `ParquetExec` registered with DataFusion.

#### Scenario: Reader exposes ParquetExec
- **WHEN** a query plan needs to scan a `ParquetFileMeta` set
- **THEN** the system constructs a `ParquetExec` whose `FileScanConfig` lists those objects and supports projection of the columns referenced in the query

### Requirement: ParquetFileMeta Partition Pruning

`ParquetFileMetaRepository::find` SHALL accept a `TimeRange` and return only files whose `time_range` overlaps it, ordered by `time_range.start`.

#### Scenario: Range filter
- **WHEN** a query restricts `_timestamp BETWEEN t0 AND t1`
- **THEN** only `ParquetFileMeta` rows with `time_range.end >= t0 AND time_range.start <= t1 AND deleted = false` are returned

### Requirement: Compactor

The compactor role SHALL periodically scan `ParquetFileMeta` rows under a target size (default 32 MiB) for each `(org, stream, date)` tuple, merge them into one parquet file, atomically swap the metadata via `ParquetFileMetaRepository::replace`, and delete the merged source objects from the object store after the swap commits.

#### Scenario: Merge happens atomically
- **WHEN** the compactor merges files `[a, b, c]` into file `d`
- **THEN** `ParquetFileMetaRepository::replace(&[a, b, c], vec![d])` is called in a single transaction; on success the objects backing `a`, `b`, `c` are removed; on failure `d`'s object is deleted and `a`, `b`, `c` remain referenceable

#### Scenario: Retention sweep
- **WHEN** a file's `time_range.end` is older than the owning `StreamDefinition.retention.days`
- **THEN** the compactor marks the `ParquetFileMeta` as `deleted = true` and removes the object on the next sweep

### Requirement: Tantivy Inverted Index

The ingester SHALL build a Tantivy index per parquet file for any field marked `indexed = true`, store it alongside the parquet in the object store (`{object_key}.tantivy.tar.zst`), and the querier SHALL use it to skip files whose index proves no matches exist for a `MATCH(field, term)` predicate.

#### Scenario: Index skips file with no match
- **WHEN** a SQL query includes `MATCH(message, "panic")` and a candidate file's tantivy index contains no posting for `panic`
- **THEN** that file is excluded from the `ParquetExec` for that query
