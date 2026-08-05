## ADDED Requirements

### Requirement: ParquetFileMeta Dump In-Process Cache

The system SHALL operate a process-level `parquet_file_meta_dump` cache, configured under `[cache.parquet_file_meta_dump]` with `capacity` and `ttl_secs` defaults `10_000` and `600` respectively. The cache key SHALL be `(org_id, stream, stream_type, partition_level, partition_key)` and the value SHALL be `Arc<Vec<ParquetFileMeta>>` holding every row of that dump parquet in deserialized form (not the bytes, not a JSON tree). `ParquetFileMetaDumpReader::read_dump_filtered` SHALL consult this cache before opening any object_store handle; on a hit the cached `Arc<Vec<ParquetFileMeta>>` SHALL be cloned (Arc clone, not deep clone) and locally filtered by the caller's `TimeRange` without touching the object store. On a miss the reader SHALL fetch via `ProductionObjectStore`, parse the columnar parquet directly into `ParquetFileMeta` rows, write the unfiltered result into the cache, and return the filtered subset.

#### Scenario: Same partition queried twice within TTL avoids object store
- **WHEN** two consecutive `ParquetFileMetaRepository::find` calls cross the same cold partition within the configured TTL
- **THEN** the second call's reader pulls `Arc<Vec<ParquetFileMeta>>` from the cache; no `ProductionObjectStore::get` is issued; `cache_parquet_file_meta_dump_hits_total` is incremented; the second call's `parquet_file_meta_dump_query_load_seconds` observation is at least 10× below the cache-miss observation

#### Scenario: Cache invalidated when dump is marked deleted
- **WHEN** `ParquetFileMetaDumpRepository::mark_deleted(object_key)` succeeds (for example during a `delete_by_time_range` rewrite)
- **THEN** the cache entry whose key matches the dump's `(org_id, stream, stream_type, partition_level, partition_key)` is dropped synchronously before the transaction commits; the next `find` for that range observes the post-rewrite state

#### Scenario: Cache invalidated when new dump is written for the same partition
- **WHEN** the worker upserts a `parquet_file_meta_dump` row for `(orgA, log_app, logs, daily, 2026-01-15)` (re-dump after orphan recovery, or a partial rewrite)
- **THEN** any existing cache entry for that exact key is dropped before the writer's transaction commits

#### Scenario: TTL expiry falls back to object store
- **WHEN** a cached entry's age exceeds `ttl_secs`
- **THEN** the next lookup treats it as miss, re-fetches from object store, and writes a fresh entry

#### Scenario: capacity = 0 disables the cache
- **WHEN** `[cache.parquet_file_meta_dump] capacity = 0`
- **THEN** the cache is bypassed (reads always miss, writes are no-ops); `ParquetFileMetaDumpReader::read_dump_filtered` always issues an object_store fetch

#### Scenario: Cache metrics appear after first lookup
- **WHEN** the cache is consulted at least once
- **THEN** `/metrics` exposes `cache_parquet_file_meta_dump_hits_total`, `cache_parquet_file_meta_dump_misses_total`, `cache_parquet_file_meta_dump_evictions_total` and the `cache_parquet_file_meta_dump_hit_ratio` gauge

#### Scenario: Eviction respects capacity
- **WHEN** the cache holds `capacity` distinct entries and a `capacity + 1`-th miss occurs
- **THEN** the least-recently-used entry is evicted, `cache_parquet_file_meta_dump_evictions_total += 1`, and the new entry is inserted
