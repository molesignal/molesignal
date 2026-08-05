# Query Capability

## Purpose

SQL（DataFusion）执行 + 多租户 planner rewrite、MATCH/Tantivy 文件级裁剪、PromQL 子集执行、Arrow Flight 分布式 querier 扇出、closed-window query_result cache、search_around 与 NDJSON 流式响应。跨集群联邦扇出见 `federated-search` capability。
## Requirements
### Requirement: SQL Query Execution

The system SHALL accept SQL queries via `POST /api/v1/query` with body `QueryRequest { language: "sql", statement, time_range, stream?, limit? }`, plan them with DataFusion against the union of `ParquetFileMeta` files within `time_range`, execute them, and return a `QueryResult { columns, rows, scanned_rows, took_ms }`.

#### Scenario: Simple aggregation completes
- **WHEN** the request is `SELECT count(*) FROM app WHERE _timestamp BETWEEN t0 AND t1`
- **THEN** the response contains one column, one row with the count, `scanned_rows` reflecting only the files actually scanned (after pruning), and `took_ms` measured from request entry to result serialization

#### Scenario: Query referencing missing stream
- **WHEN** the SQL references a table that does not exist for the org
- **THEN** the response is `400 Bad Request` with `{ "error": "stream not found: <name>" }`

#### Scenario: Limit clamps result size
- **WHEN** the request includes `limit: 1000` for a query that would otherwise return more
- **THEN** only the first 1000 rows are returned and `scanned_rows` is unchanged

### Requirement: Multi-Tenant SQL Planner Rewrite

Before logical planning, every SQL query SHALL pass through a `RewriteTableNamesPass` that rewrites every `TableScan(stream_name)` (including those nested inside CTEs, joins, subqueries, and UNIONs) to an equivalent `TableScan(stream_name) WHERE org_id = '<auth_ctx.org_id>'`. The rewrite SHALL also reject queries that reference any stream not owned by the caller's org.

#### Scenario: Direct cross-org reference rejected
- **WHEN** a user from `orgA` issues `SELECT * FROM app` and `app` belongs to `orgB`
- **THEN** the response is `403 Forbidden` with `{ "error": "stream not found: app" }` (using 403 vs 404 phrasing harmonized with existing IAM isolation semantics)

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

### Requirement: Query Authorization

The query handler SHALL require `Permission::StreamRead` on the target stream(s); cross-org access is rejected.

#### Scenario: Cross-org query rejected
- **WHEN** the caller is a member of org A but the SQL references a stream owned by org B
- **THEN** the response is `403 Forbidden` and the query is not planned

### Requirement: Multi-Stream SQL Search

The system SHALL support SQL that references multiple streams in a single statement (e.g., `SELECT a.trace_id, b.host FROM app_logs a JOIN host_metrics b ON a.host = b.host`). The planner SHALL collect all referenced streams, fetch each one's `ParquetFileMeta` set independently, apply the multi-tenant rewrite to each scan, and let DataFusion execute the join over the union.

#### Scenario: Two-stream JOIN executes

- **WHEN** a user submits `SELECT a.level, count(*) FROM app_logs a JOIN host_metrics b ON a.host = b.host WHERE _timestamp BETWEEN ... GROUP BY a.level`
- **THEN** the response contains aggregated rows; each scan was org-scoped independently; `scanned_rows` reflects both streams' files

#### Scenario: Cross-org reference in JOIN rejected

- **WHEN** the JOIN references a stream belonging to another org
- **THEN** the rewrite pass returns 403 with body `{ "error": "stream not found: <name>" }`

### Requirement: Search Jobs Auto-Conversion

When a `POST /api/v1/query` request's predicted scan exceeds a threshold (`[querier].auto_async_threshold_rows`, default 10M) or carries `Prefer: respond-async` header, the system SHALL automatically convert to an async search job (see `search-jobs` capability) and return `202 Accepted` with `{ "job_id": "<ksuid>", "monitor": "/api/v1/query/jobs/<id>" }` instead of executing inline.

#### Scenario: Large query auto-converts

- **WHEN** a query's planner estimate exceeds `auto_async_threshold_rows`
- **THEN** response is `202` with `job_id`; the caller polls via `/jobs/<id>` to retrieve

#### Scenario: Header-forced async

- **WHEN** the request carries `Prefer: respond-async` regardless of size
- **THEN** the system always converts to async, even for small queries

### Requirement: SQL Reference Extraction Via AST

`infra::query::parser::extract_referenced_tables(stmt)` SHALL parse the SQL statement using `sqlparser::ast` and return a deduplicated, ordered list of `TableRef { name, alias?, schema? }` referencing only **base tables**:
- CTE inner table references SHALL be included (CTE body's `FROM <real_table>`)
- CTE outer references SHALL be excluded (a SELECT `FROM cte_name` does NOT yield `cte_name` as a base table)
- Subqueries SHALL be walked recursively
- Quoted identifiers (`"my-table"`) are unquoted into the bare name
- Schema-qualified `db.schema.table` reduces to `table` for current single-schema design

#### Scenario: CTE body and outer reference

- **WHEN** SQL = `WITH high_lat AS (SELECT * FROM traces WHERE duration_us > 1000) SELECT * FROM high_lat JOIN services s ON high_lat.svc = s.name`
- **THEN** extracted base tables = `["services", "traces"]` (NOT `high_lat`)

#### Scenario: Quoted identifier unquoted

- **WHEN** SQL = `SELECT * FROM "weird-stream"`
- **THEN** extracted = `["weird-stream"]`

#### Scenario: Schema-qualified reduces

- **WHEN** SQL = `SELECT * FROM analytics.logs`
- **THEN** extracted = `["logs"]`

#### Scenario: Multiple JOINs with aliases

- **WHEN** SQL = `SELECT * FROM logs l JOIN metrics m ON l.ts = m.ts JOIN traces t ON l.trace_id = t.trace_id`
- **THEN** extracted = `["logs", "metrics", "traces"]` (alphabetical or insertion order; both acceptable)

#### Scenario: Subquery FROM walked

- **WHEN** SQL = `SELECT * FROM (SELECT id FROM logs) sub JOIN traces ON sub.id = traces.id`
- **THEN** extracted contains both `logs` and `traces`

#### Scenario: Self-join deduplicated

- **WHEN** SQL = `SELECT * FROM logs a JOIN logs b ON a.id = b.parent_id`
- **THEN** extracted = `["logs"]` (single entry)

#### Scenario: Lateral subquery walked

- **WHEN** SQL contains `FROM logs JOIN LATERAL (SELECT * FROM metrics WHERE ts = logs.ts)`
- **THEN** extracted contains both `logs` and `metrics`

#### Scenario: Invalid SQL surfaces error

- **WHEN** SQL is syntactically invalid (e.g. `SELEC * FROM`)
- **THEN** `extract_referenced_tables` returns `Err(Error::invalid("sqlparser: ..."))` rather than producing wrong results

### Requirement: DataFusion Engine Uses AST Extraction

`DataFusionEngine::execute` SHALL replace the regex-based table parser with `extract_referenced_tables`. The MemTable registration loop behavior MUST remain the same:
- Primary `StreamHint` is registered first
- Additional referenced tables are resolved via `StreamRepository::get` across 4 stream types; misses are skipped (DataFusion will surface "table not found" if SQL uses them)

#### Scenario: CTE no longer triggers spurious table lookup

- **WHEN** SQL = `WITH x AS (SELECT * FROM logs) SELECT * FROM x`
- **THEN** the engine registers `logs` MemTable once and does NOT issue a `StreamRepository::get(org, "x", ...)` lookup (old regex would)

### Requirement: Rewrite Framework Available

`infra::query::rewrite::enforce_org_isolation(stmt: &str, org_id: &Id) -> Result<String>` SHALL be exposed as a public function. Current implementation: passthrough (returns `stmt` unchanged) with a doc comment explaining the future contract. The function exists so future PRs (when `_org_id` column is added to streams) can drop in actual rewriting without API churn for the caller.

#### Scenario: Passthrough preserves SQL semantics

- **WHEN** `enforce_org_isolation("SELECT * FROM logs", &org)` is called today
- **THEN** the function returns `"SELECT * FROM logs"` unchanged (function is reserved for future use)
