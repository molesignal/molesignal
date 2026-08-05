## ADDED Requirements

### Requirement: Multi-Level Process Cache

The system SHALL operate three independent in-process caches (`parquet_file_meta`, `parquet_meta`, `query_result`), each backed by `moka` LRU with TTL and capacity limits configured under the `[caching]` settings section.

#### Scenario: parquet_file_meta cache hit avoids DB roundtrip
- **WHEN** the same `(org, stream, stream_type, time_bucket_hour)` is requested twice within the configured TTL
- **THEN** the second lookup returns the cached `Vec<ParquetFileMeta>` without issuing a SQL query, and `cache_parquet_file_meta_hits_total` is incremented

#### Scenario: parquet_meta cache reuses ParquetMetaData
- **WHEN** the same parquet file is referenced by two concurrent queries and the metadata footer was already fetched once
- **THEN** the second query reuses the cached `Arc<ParquetMetaData>` without re-fetching the file footer from the object store

#### Scenario: query_result cache only stores closed-window queries
- **WHEN** an SQL query's `time_range.end` is older than `now - 5min` and the language is `sql`
- **THEN** the result is stored in the `query_result` cache for `caching.query_result.ttl_secs` (default 60s) and subsequent identical requests return the cached body with `cache_hit: true`

#### Scenario: query_result cache skips open-window queries
- **WHEN** an SQL query's `time_range.end >= now - 5min` OR the language is `promql`
- **THEN** the cache is bypassed (neither read nor written) and `cache_query_result_bypassed_total` is incremented

### Requirement: Parquet Disk Cache

When `[caching.disk_cache] enabled = true`, the parquet reader SHALL first check the local disk cache under `caching.disk_cache.dir` (default `./data/cache/parquet`) for an entry keyed by `sha256(object_key)`. A hit SHALL serve directly from disk; a miss SHALL fetch from the object store, store the file under the cache dir (best-effort, errors logged not fatal), and serve. The cache SHALL evict via LRU when total disk size exceeds `caching.disk_cache.max_size_gb` (default 10).

#### Scenario: Cache hit avoids object store fetch
- **WHEN** the same parquet is read twice and `disk_cache.enabled = true`
- **THEN** the second read issues zero object-store GETs; `cache_parquet_disk_hits_total += 1`

#### Scenario: Eviction respects max_size_gb
- **WHEN** total cache occupancy would exceed `max_size_gb` after a miss-fill
- **THEN** the least-recently-used entries are deleted until occupancy is below the limit; `cache_parquet_disk_evictions_total` increments per removal

#### Scenario: Cache miss when disabled
- **WHEN** `disk_cache.enabled = false`
- **THEN** all reads go straight to the object store and the cache directory is not touched

### Requirement: Cache Invalidation on Write

`ParquetFileMetaRepository::insert`, `replace`, and `mark_deleted` SHALL invalidate every `parquet_file_meta` cache entry whose key prefix matches `(org, stream, stream_type)` synchronously before returning success.

#### Scenario: New file invalidates the cache prefix
- **WHEN** `ParquetFileMetaRepository::insert` succeeds for `(orgA, log_app, logs, …)`
- **THEN** every `parquet_file_meta` cache entry under `(orgA, log_app, logs, *)` is dropped, and the next query observes the new file

#### Scenario: Compactor replace invalidates both old and new
- **WHEN** `ParquetFileMetaRepository::replace(removed, added)` commits
- **THEN** entries for the affected `(org, stream, stream_type)` keys are invalidated; subsequent queries see only `added` and not the merged-away `removed`

### Requirement: Cache Metrics Exposed via `/metrics`

Each cache SHALL export `cache_<level>_hits_total`, `cache_<level>_misses_total`, and `cache_<level>_evictions_total` Counter metrics, plus a `cache_<level>_hit_ratio` Gauge updated every Prometheus scrape.

#### Scenario: Hit ratio gauge reflects recent traffic
- **WHEN** 80 hits and 20 misses are recorded against `parquet_file_meta` within the scrape window
- **THEN** the next `/metrics` scrape reports `cache_parquet_file_meta_hit_ratio 0.80`
