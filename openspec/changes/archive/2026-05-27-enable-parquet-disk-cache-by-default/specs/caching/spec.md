## MODIFIED Requirements

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

### Requirement: Cache Metrics Exposed via `/metrics`

Each cache SHALL export `cache_<level>_hits_total`, `cache_<level>_misses_total`, and `cache_<level>_evictions_total` Counter metrics, plus a `cache_<level>_hit_ratio` Gauge updated every Prometheus scrape. The Parquet Disk Cache SHALL use `<level> = parquet_disk` and emit `cache_parquet_disk_hits_total`, `cache_parquet_disk_misses_total`, `cache_parquet_disk_evictions_total`, and `cache_parquet_disk_hit_ratio`.

#### Scenario: Hit ratio gauge reflects recent traffic
- **WHEN** 80 hits and 20 misses are recorded against `parquet_file_meta` within the scrape window
- **THEN** the next `/metrics` scrape reports `cache_parquet_file_meta_hit_ratio 0.80`

#### Scenario: Parquet disk cache metrics appear after first lookup
- **WHEN** the process starts with the Parquet Disk Cache effectively enabled and at least one parquet read has occurred
- **THEN** `/metrics` exposes `cache_parquet_disk_hits_total`, `cache_parquet_disk_misses_total`, `cache_parquet_disk_evictions_total`, and `cache_parquet_disk_hit_ratio`
