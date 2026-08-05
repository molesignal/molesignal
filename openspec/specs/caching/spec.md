# Caching Capability

## Purpose

进程内多级缓存（parquet_file_meta / parquet_meta / query_result）与 parquet 本地磁盘缓存，配合写时失效与 Prometheus 命中率指标，减少元数据回源与对象存储拉取。
## Requirements
### Requirement: Multi-Level Process Cache

The system SHALL operate three independent in-process caches (`parquet_file_meta`, `parquet_meta`, `query_result`), each backed by `moka` LRU with TTL and capacity limits configured under the `[cache]` settings section.

#### Scenario: parquet_file_meta cache hit avoids DB roundtrip
- **WHEN** the same `(org, stream, stream_type, time_bucket_hour)` is requested twice within the configured TTL
- **THEN** the second lookup returns the cached `Vec<ParquetFileMeta>` without issuing a SQL query, and `cache_parquet_file_meta_hits_total` is incremented

#### Scenario: parquet_meta cache reuses ParquetMetaData
- **WHEN** the same parquet file is referenced by two concurrent queries and the metadata footer was already fetched once
- **THEN** the second query reuses the cached `Arc<ParquetMetaData>` without re-fetching the file footer from the object store

#### Scenario: query_result cache only stores closed-window queries
- **WHEN** an SQL query's `time_range.end` is older than `now - 5min` and the language is `sql`
- **THEN** the result is stored in the `query_result` cache for `cache.query_result.ttl_secs` (default 60s) and subsequent identical requests return the cached body with `cache_hit: true`

#### Scenario: query_result cache skips open-window queries
- **WHEN** an SQL query's `time_range.end >= now - 5min` OR the language is `promql`
- **THEN** the cache is bypassed (neither read nor written) and `cache_query_result_bypassed_total` is incremented

### Requirement: Parquet Disk Cache

The system SHALL ship a Parquet Disk Cache configured under `[cache.disk_cache]` with defaults `enabled = true`, `dir = "./data/cache/parquet"`, `max_size_gb = 10`. When effectively enabled (`enabled = true` AND `max_size_gb > 0`), the parquet reader SHALL first check the local disk cache (keyed by `sha256(object_key)`); a hit SHALL serve directly from disk; a miss SHALL fetch from the object store, store the file under the cache directory (best-effort, errors logged not fatal), and serve. Bootstrap SHALL `create_dir_all` the configured directory at startup so the cache directory does not have to pre-exist. The cache SHALL evict via LRU when total disk size exceeds `max_size_gb`.

#### Scenario: Default deployment enables the cache automatically
- **WHEN** the process starts with the shipped default `conf/config.toml` and no `[cache.disk_cache]` override
- **THEN** `ParquetDiskCache` is instantiated, the cache directory `./data/cache/parquet` is created if missing, and subsequent parquet reads consult it first

#### Scenario: Cache hit avoids object store fetch
- **WHEN** the same parquet is read twice with `enabled = true` and `max_size_gb > 0`
- **THEN** the second read issues zero object-store GETs; `cache_parquet_disk_hits_total` is incremented by 1

#### Scenario: Eviction respects max_size_gb
- **WHEN** total cache occupancy would exceed `max_size_gb` after a miss-fill
- **THEN** the least-recently-used entries are deleted until occupancy is below the limit; `cache_parquet_disk_evictions_total` increments per removal

#### Scenario: Cache miss when disabled
- **WHEN** `enabled = false`
- **THEN** `ParquetDiskCache` is not instantiated, all reads go straight to the object store, and the cache directory is not touched

#### Scenario: max_size_gb = 0 is equivalent to disabled
- **WHEN** `enabled = true` and `max_size_gb = 0`
- **THEN** `ParquetDiskCache` is not instantiated and behavior matches `enabled = false`

#### Scenario: Missing cache directory is auto-created
- **WHEN** the configured `dir` does not exist at startup and the cache is effectively enabled
- **THEN** bootstrap calls `create_dir_all(dir)` before instantiating the cache; startup does not abort on `NotFound`

### Requirement: Cache Invalidation on Write

`ParquetFileMetaRepository::insert`, `replace`, and `mark_deleted` SHALL invalidate every `parquet_file_meta` cache entry whose key prefix matches `(org, stream, stream_type)` synchronously before returning success.

#### Scenario: New file invalidates the cache prefix
- **WHEN** `ParquetFileMetaRepository::insert` succeeds for `(orgA, log_app, logs, …)`
- **THEN** every `parquet_file_meta` cache entry under `(orgA, log_app, logs, *)` is dropped, and the next query observes the new file

#### Scenario: Compactor replace invalidates both old and new
- **WHEN** `ParquetFileMetaRepository::replace(removed, added)` commits
- **THEN** entries for the affected `(org, stream, stream_type)` keys are invalidated; subsequent queries see only `added` and not the merged-away `removed`

### Requirement: Cache Metrics Exposed via `/metrics`

Each cache SHALL export `cache_<level>_hits_total`, `cache_<level>_misses_total`, and `cache_<level>_evictions_total` Counter metrics, plus a `cache_<level>_hit_ratio` Gauge updated every Prometheus scrape. The Parquet Disk Cache SHALL use `<level> = parquet_disk` and emit `cache_parquet_disk_hits_total`, `cache_parquet_disk_misses_total`, `cache_parquet_disk_evictions_total`, and `cache_parquet_disk_hit_ratio`.

#### Scenario: Hit ratio gauge reflects recent traffic
- **WHEN** 80 hits and 20 misses are recorded against `parquet_file_meta` within the scrape window
- **THEN** the next `/metrics` scrape reports `cache_parquet_file_meta_hit_ratio 0.80`

#### Scenario: Parquet disk cache metrics appear after first lookup
- **WHEN** the process starts with the Parquet Disk Cache effectively enabled and at least one parquet read has occurred
- **THEN** `/metrics` exposes `cache_parquet_disk_hits_total`, `cache_parquet_disk_misses_total`, `cache_parquet_disk_evictions_total`, and `cache_parquet_disk_hit_ratio`

### Requirement: Tantivy Result Cache

The system SHALL operate a process-level `tantivy_result` cache, configured under `[cache.tantivy_result]` with `capacity` and `ttl_secs` defaults `1_000_000` and `600` respectively. The cache key SHALL be `(index_object_key, field, term)` where `index_object_key` is the canonical puffin sidecar key `files/{org}/index/{stream}_{stream_type}/{YYYY}/{MM}/{DD}/00/{ksuid}.ttv`; the value SHALL be the `count: u64` returned by `IndexHandle::count_term`. `TantivyPruner::prune` SHALL consult this cache before opening any `IndexHandle`; on a hit it SHALL skip the tantivy invocation entirely.

#### Scenario: Same predicate against same archive avoids tantivy on second call
- **WHEN** `TantivyPruner::prune` is invoked twice with identical `(index_object_key, field, term)` within the configured TTL
- **THEN** the second invocation does not call `IndexHandle::count_term` and `cache_tantivy_result_hits_total` is incremented

#### Scenario: Result cache invalidated when archive replaced
- **WHEN** `ParquetFileMetaRepository::replace(removed, added)` commits and `removed` contains a ParquetFileMeta whose `object_key` has an associated puffin sidecar
- **THEN** every `tantivy_result` cache entry whose `index_object_key` corresponds to the removed ParquetFileMeta is dropped before the next `prune` runs

#### Scenario: Result cache invalidated when archive deleted by retention
- **WHEN** `ParquetFileMetaRepository::mark_deleted(ids)` succeeds for ParquetFileMetas with puffin sidecars
- **THEN** the corresponding `tantivy_result` entries are dropped synchronously before the call returns

#### Scenario: Cache miss falls through to tantivy
- **WHEN** the cache key is not present
- **THEN** `IndexHandle::count_term` is called, the returned count is written into the cache, and `cache_tantivy_result_misses_total` is incremented

#### Scenario: capacity = 0 disables the cache
- **WHEN** `cache.tantivy_result.capacity = 0`
- **THEN** the cache is bypassed (reads always miss, writes are no-ops); `TantivyPruner::prune` always calls `IndexHandle::count_term` directly

### Requirement: Tantivy Footer Cache

The system SHALL operate a process-level `tantivy_footer` cache, configured under `[cache.tantivy_footer]` with `capacity` and `ttl_secs` defaults `100_000` and `3600` respectively. The cache key SHALL be `index_object_key` (the canonical puffin sidecar key) and the value SHALL be an `Arc<TantivyFooter>` containing **only** `{ puffin_meta: Arc<PuffinMeta>, footer_payload_bytes: Bytes, schema: tantivy::schema::Schema }` — i.e. the parsed puffin footer (typically a few KB) plus the raw payload bytes needed to re-open an `IndexHandle` without re-fetching the footer from the object store. `TantivyFooter` SHALL NOT carry the full sidecar archive bytes; only the footer payload. When the `IndexHandle` cache misses, archive opening SHALL consult the footer cache first; on a hit the parsed footer is reused and only the segment payload blobs actually touched by the query are fetched.

#### Scenario: Re-open after IndexHandle eviction reuses cached footer
- **WHEN** the `IndexHandle` cache has evicted an entry but the corresponding `tantivy_footer` entry is still live
- **THEN** the next `prune` call rebuilds the `IndexHandle` using the cached footer; the puffin footer is not re-parsed from `object_store` bytes; `cache_tantivy_footer_hits_total` is incremented; `tantivy_puffin_footer_bytes_read_total` does NOT increase

#### Scenario: Footer cache value size is bounded
- **WHEN** a 20 MiB sidecar's footer cache entry is constructed
- **THEN** the cached `TantivyFooter` reports `size_bytes()` ≤ 64 KiB (parsed `PuffinMeta` JSON + footer payload bytes + estimated schema overhead); the full 20 MiB is NOT held in the cache

#### Scenario: Footer cache invalidated on archive replace
- **WHEN** `ParquetFileMetaRepository::replace(removed, added)` commits
- **THEN** every `tantivy_footer` entry whose `index_object_key` corresponds to a removed ParquetFileMeta is dropped

#### Scenario: Footer cache invalidated on archive delete
- **WHEN** `ParquetFileMetaRepository::mark_deleted(ids)` succeeds
- **THEN** the corresponding `tantivy_footer` entries are dropped synchronously before the call returns

#### Scenario: capacity = 0 disables the cache
- **WHEN** `cache.tantivy_footer.capacity = 0`
- **THEN** the cache is bypassed; archive opening always re-parses the footer from object_store bytes

#### Scenario: Cache errors do not block queries
- **WHEN** the footer cache returns an error (e.g., poisoned lock)
- **THEN** archive opening falls through to the full parse path and `cache_tantivy_footer_errors_total` is incremented; the query still completes

### Requirement: Tantivy Cache Metrics Exposed via `/metrics`

The `tantivy_result` and `tantivy_footer` caches SHALL each export Prometheus metrics following the existing `cache_<level>_*` convention: `cache_tantivy_result_hits_total`, `cache_tantivy_result_misses_total`, `cache_tantivy_result_evictions_total`, `cache_tantivy_result_hit_ratio`, `cache_tantivy_footer_hits_total`, `cache_tantivy_footer_misses_total`, `cache_tantivy_footer_evictions_total`, `cache_tantivy_footer_hit_ratio`. Metric **names** are unchanged from the pre-puffin design even though the cache key labelling (`archive_key` → `index_object_key`) changed; metric labels SHALL NOT carry the key itself (no high-cardinality label leak).

#### Scenario: Hit ratio gauge reflects recent traffic for tantivy_result
- **WHEN** 60 hits and 40 misses are recorded against `tantivy_result` within the scrape window
- **THEN** the next `/metrics` scrape reports `cache_tantivy_result_hit_ratio 0.60`

#### Scenario: All new metrics appear once caches are active
- **WHEN** the process starts with `tantivy_result.capacity > 0` and `tantivy_footer.capacity > 0` and at least one `prune` call has executed
- **THEN** `/metrics` exposes all eight `cache_tantivy_{result,footer}_*` metrics

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
