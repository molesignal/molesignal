## MODIFIED Requirements

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
