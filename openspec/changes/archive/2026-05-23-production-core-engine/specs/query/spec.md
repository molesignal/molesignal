## ADDED Requirements

### Requirement: Multi-Tenant SQL Planner Rewrite

Before logical planning, every SQL query SHALL pass through a `RewriteTableNamesPass` that rewrites every `TableScan(stream_name)` (including those nested inside CTEs, joins, subqueries, and UNIONs) to an equivalent `TableScan(stream_name) WHERE org_id = '<auth_ctx.org_id>'`. The rewrite SHALL also reject queries that reference any stream not owned by the caller's org.

#### Scenario: Direct cross-org reference rejected
- **WHEN** a user from `orgA` issues `SELECT * FROM app` and `app` belongs to `orgB`
- **THEN** the response is `403 Forbidden` with `{ "error": "stream not found: app" }` (using 403 vs 404 phrasing harmonized with existing identity semantics)

#### Scenario: Subquery cross-org reference rejected
- **WHEN** the query is `SELECT * FROM (SELECT * FROM app)` and `app` belongs to `orgB`
- **THEN** the rewrite pass aborts with `Error::Forbidden`, no scan is registered, and no candidate `ParquetFileMeta` is fetched

#### Scenario: Multi-stream join scoped per org
- **WHEN** a user joins streams `app` and `nginx` they own
- **THEN** both table scans are rewritten with `WHERE org_id = '<user_org>'` and the planner proceeds normally

### Requirement: MATCH Predicate Tantivy Pruning

The query planner SHALL detect `MATCH(field, term)` predicates in the `WHERE` clause, consult the `caching::parquet_meta`-style Tantivy index cache for each candidate `ParquetFileMeta`, and drop files whose index reports zero matches BEFORE the `ParquetExec` is constructed.

#### Scenario: All matching files pruned
- **WHEN** a query has `WHERE MATCH(message, 'NotARealString')` and no file's Tantivy index contains the term
- **THEN** the planner registers a `ParquetExec` with an empty file list and returns `scanned_rows = 0` without touching the object store further

#### Scenario: Partial pruning
- **WHEN** 10 files are candidates and only 3 have postings for the term
- **THEN** `ParquetExec` scans those 3 files only, `tantivy_pruned_files_total` increments by 7, and `scanned_files = 3` appears in the response

### Requirement: Query Result Cache

The query handler SHALL consult the `caching::query_result` LRU before planning when the query language is `sql` AND the request's `time_range.end <= now - 5min`. A miss runs the full pipeline and stores the response; a hit returns the cached body with `cache_hit: true`.

#### Scenario: Closed-window query cached
- **WHEN** the same SQL (same `statement`, same `time_range`, same `org_id`) is issued twice within `caching.query_result.ttl_secs`
- **THEN** the second call returns immediately from cache, `cache_query_result_hits_total` increments, and the response body has `cache_hit: true`

#### Scenario: Open-window query bypasses cache
- **WHEN** `time_range.end >= now - 5min`
- **THEN** the cache is not consulted, the cache is not populated, and the response body has `cache_hit: false`

### Requirement: Search Around Endpoint

The system SHALL expose `POST /api/v1/query/search_around` with body `{ stream, event_pointer: { _timestamp, hash }, before: u32, after: u32, filter?: SQL_WHERE }` returning up to `before + after` events surrounding the pointed event, ordered by `_timestamp ASC`. `before` and `after` MUST each be ≤ `query.search_around_max_each` (default 500). The `hash` field disambiguates events with identical timestamps (uses blake3 of the record's serialized form).

#### Scenario: 50 before / 50 after returned
- **WHEN** a user requests `{ stream: "app", event_pointer: ..., before: 50, after: 50 }`
- **THEN** the response is `{ items: [...], total: N <= 101 }` ordered ascending

#### Scenario: Pointer not found
- **WHEN** the `(_timestamp, hash)` pointer cannot be located in the stream
- **THEN** the response is `404 Not Found` with `{ "error": "event_pointer not found" }`

### Requirement: Streaming Query Responses

When the request carries `Accept: application/x-ndjson` (or `text/event-stream`), the query handler SHALL stream `RecordBatch`-derived NDJSON (or SSE) chunks as they arrive from the executor, terminating with a final `{"meta": { took_ms, scanned_rows, cache_hit, degraded_clusters? }}` chunk. Streaming SHALL bypass the `query_result` cache (neither read nor write) since partial results invalidate the cache contract.

#### Scenario: NDJSON streaming yields first rows quickly
- **WHEN** a query with `LIMIT 10000` is sent with `Accept: application/x-ndjson` and the executor produces RecordBatches of 1024 rows each
- **THEN** the client observes the first batch chunk within ≤ 500ms of header bytes; subsequent batches stream until completion

#### Scenario: Default content-type returns single JSON
- **WHEN** the same query has `Accept: application/json` (or omitted)
- **THEN** the existing buffered response shape applies and caching rules are honored

## MODIFIED Requirements

### Requirement: PromQL Query Entry Point

The system SHALL accept `language: "promql"` requests and dispatch them to a `PromQLEngine` implementing instant and range queries over metrics streams. The engine SHALL support at minimum these functions: `rate`, `increase`, `sum`, `avg`, `min`, `max`, `count`, `histogram_quantile`. Unsupported functions SHALL return `400 Bad Request` with `{ "error": "promql function not yet supported: <name>" }`.

#### Scenario: Supported function executes
- **WHEN** the request is `{ "language": "promql", "statement": "rate(http_requests_total[5m])", "time_range": {...} }`
- **THEN** the engine evaluates the expression and returns a series matrix in `QueryResult.rows` shaped as `(timestamp, label_set_json, value)`

#### Scenario: Unsupported function rejected with explicit error
- **WHEN** the request statement is `holt_winters(metric[1h], 0.5, 0.5)`
- **THEN** the response is `400 Bad Request` with `{ "error": "promql function not yet supported: holt_winters" }` and no scan is issued

#### Scenario: Range query returns time-aligned matrix
- **WHEN** the request includes `time_range.start`, `time_range.end`, and a `step` field
- **THEN** the engine evaluates the expression at each `start + n * step` instant and returns rows whose timestamps are exactly those instants

### Requirement: Distributed Querier via Arrow Flight

When the cluster registry reports two or more healthy `Querier` nodes, the querier handling the request (the coordinator) SHALL split the candidate `ParquetFileMeta` list across peers by a consistent hash of `object_key`, dispatch a `QueryShard { sql, parquet_file_metas, projection, time_range }` Ticket via `arrow_flight::do_get` to each peer, and merge the resulting `RecordBatch` streams locally. The coordinator SHALL re-run the full SQL (final aggregation included) over the UNION of peer outputs; peers SHALL execute only scan + projection + WHERE on their assigned files.

#### Scenario: Two-node fan-out
- **WHEN** two queriers are healthy and 100 files match the request
- **THEN** roughly half the files are scanned on each node (per consistent hash) and the requesting node merges both Flight streams before running the final SQL aggregation

#### Scenario: Single-node fallback
- **WHEN** only one querier is healthy in the registry
- **THEN** the request is executed locally without invoking Arrow Flight

#### Scenario: Peer failure mid-stream
- **WHEN** a peer returns an error mid-`do_get`
- **THEN** the coordinator cancels the other in-flight `do_get` calls, returns `500 Internal` with `{ "error": "querier peer failed: <node_id>" }`, and `querier_peer_errors_total{peer=…}` increments
