## ADDED Requirements

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
