## MODIFIED Requirements

### Requirement: Tantivy Inverted Index

The ingester SHALL build a Tantivy index per parquet file for any field marked `indexed = true`, serialize the index as a **Puffin v1 file** (multi-blob single-object container; one blob per tantivy segment file plus one `O2TtvFooterV1` footer-cache blob), and store it at the canonical sidecar key `files/{org}/index/{stream}_{stream_type}/{YYYY}/{MM}/{DD}/00/{ksuid}.ttv` derived from the parquet object key via `convert_parquet_file_name_to_tantivy_file`. The querier SHALL use the puffin sidecar to skip files whose index proves no matches exist for a `MATCH(field, term)` predicate. Loading a puffin sidecar SHALL go through `PuffinDirReader::from_object_store`, fetching only the footer (≥ 12 bytes + payload) up front; subsequent tantivy reads SHALL translate into per-blob sub-range `get_range` calls so the full sidecar is never downloaded as a single blob. The legacy `tar+zstd`-encoded sidecar at `{object_key}.tantivy.tar.zst` SHALL no longer be written or read; missing puffin sidecars SHALL be handled by the existing `tantivy_missing_archive_total` fall-through.

#### Scenario: Index skips file with no match
- **WHEN** a SQL query includes `MATCH(message, 'panic')` and a candidate file's tantivy index contains no posting for `panic`
- **THEN** that file is excluded from the `ParquetExec` for that query and a `tantivy_pruned_files_total` counter increments

#### Scenario: Missing puffin sidecar falls back to full scan
- **WHEN** a `MATCH` predicate targets a field but the parquet has no companion `.ttv` puffin sidecar (e.g. field was not yet `indexed=true` when the file was written, or the legacy `.tantivy.tar.zst` exists but the puffin sidecar does not)
- **THEN** that file is kept in the candidate set, the `MATCH` is evaluated row-by-row by DataFusion, and `tantivy_missing_archive_total` increments; **the legacy `.tantivy.tar.zst` SHALL NOT be downloaded or parsed under any circumstance**

#### Scenario: Sidecar key follows puffin canonical layout
- **WHEN** an ingester flushes a parquet at key `orgA/logs/log_app/2026-01-15/abc123.parquet`
- **THEN** the puffin sidecar SHALL be uploaded to `files/orgA/index/log_app_logs/2026/01/15/00/abc123.ttv`; no other sidecar location SHALL be created

#### Scenario: Querier loads sidecar via footer only
- **WHEN** the querier opens a 12 MiB puffin sidecar to check a single `(field, term)` predicate
- **THEN** the first `object_store` access is a `get_range` for the trailing 12 bytes (footer tail) followed by one `get_range` for the footer payload; the full 12 MiB is NOT downloaded; subsequent tantivy reads issue per-blob `get_range` calls only for the segment files actually touched

#### Scenario: Index sidecar size bounded
- **WHEN** the ingester writes a Tantivy puffin sidecar for a parquet that is `S` bytes
- **THEN** the sidecar size MUST be less than `S * 0.20`; if it exceeds 20% the writer logs a warning with the field names and proceeds (no hard fail)
