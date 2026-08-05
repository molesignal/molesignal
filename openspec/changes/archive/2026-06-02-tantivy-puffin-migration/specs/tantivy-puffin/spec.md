## ADDED Requirements

### Requirement: Puffin v1 File Format

The system SHALL implement a Puffin v1 binary file format conforming to the layout `MAGIC(4) | <blob bytes>… | MAGIC(4) | PAYLOAD(JSON) | PAYLOAD_SIZE(4 LE) | FLAGS(4 LE) | MAGIC(4)` where `MAGIC = [0x50, 0x46, 0x41, 0x31]` (`PFA1`). `PAYLOAD` SHALL be a JSON-encoded `PuffinMeta` containing `blobs: Vec<BlobMetadata>` and `properties: HashMap<String, String>`. Every `BlobMetadata` SHALL include `{ blob_type, fields, snapshot_id, sequence_number, offset, length, compression_codec, properties }`. The implementation SHALL live in crate `tantivy_utils` under `puffin/` and expose `PuffinBytesWriter` (append blob, set properties, finish) and `PuffinBytesReader` (parse footer, read blob bytes by range).

#### Scenario: Round-trip preserves blob ordering
- **WHEN** three blobs `b0, b1, b2` are appended in order and the file is serialized then re-parsed
- **THEN** `PuffinMeta.blobs` yields three entries with `offset[0] < offset[1] < offset[2]`, each `length` matches the original input, and reading each blob's bytes returns the original payload

#### Scenario: Footer is fixed 12 bytes
- **WHEN** a puffin file of any size is produced
- **THEN** the final 12 bytes encode `payload_size: u32 LE | flags: u32 LE | MAGIC`; the byte at offset 0 is the leading MAGIC

#### Scenario: Reader rejects mismatched magic
- **WHEN** `PuffinBytesReader::parse_footer` is invoked on bytes whose trailing 4 bytes are not `PFA1`
- **THEN** parsing returns an error tagged `Footer MAGIC mismatch`; no partial state is exposed

#### Scenario: Blob read uses sub-range
- **WHEN** `PuffinBytesReader::read_blob_bytes(meta, Some(0..1024))` is invoked on a blob whose absolute offset in the file is `o`
- **THEN** the underlying object_store call is `get_range(o..o+1024)`, not a full-object `get`

### Requirement: Puffin-Backed Tantivy Directory (Read)

The system SHALL provide a `PuffinDirReader` implementing `tantivy::directory::Directory` that maps tantivy file lookups to puffin blob reads. `PuffinDirReader::from_object_store(store, path, size)` SHALL fetch the puffin footer via two `get_range` calls (footer tail then payload), build a `PathBuf → Arc<BlobMetadata>` map keyed by each blob's `properties["blob_tag"]`, and return a `Directory` that returns one `PuffinSliceHandle` per `get_file_handle` call. `PuffinSliceHandle::read_bytes_async(byte_range)` SHALL translate `byte_range` into the absolute object range `[blob.offset + byte_range.start, blob.offset + byte_range.end)` and issue a single `object_store::get_range`. Synchronous `read_bytes` SHALL return an error indicating sync I/O is not supported. The directory SHALL be read-only: `atomic_write`, `delete`, `open_write`, `sync_directory` SHALL panic via `unimplemented!`. Missing tantivy segment files (e.g. `.fieldnorm` not present in this index) SHALL be served from a static "empty puffin directory" bundled with the crate so that tantivy never sees `FileDoesNotExist` for files it expects to exist on every segment.

#### Scenario: Tantivy read translates to blob sub-range get_range
- **WHEN** tantivy queries a `.term` segment file and requests bytes 0..4096
- **THEN** the underlying `object_store::ObjectStore::get_range` is called with absolute range `[blob.offset, blob.offset + 4096)`, exactly once; `tantivy_puffin_blob_range_reads_total` increments by 1

#### Scenario: Missing optional segment file falls back to empty
- **WHEN** tantivy opens a segment and asks for a `.fieldnorm` file that was not produced (e.g. fields configured without norms)
- **THEN** `PuffinDirReader::get_file_handle` returns a handle backed by the bundled empty-directory bytes; tantivy proceeds without error

#### Scenario: Read-only writes panic
- **WHEN** any caller invokes `PuffinDirReader::open_write` or `atomic_write`
- **THEN** the call panics with `unimplemented!("read-only")`

### Requirement: Puffin-Backed Tantivy Directory (Write)

The system SHALL provide a `PuffinDirWriter` implementing `tantivy::directory::Directory` backed by a tempdir `MmapDirectory`. `PuffinDirWriter::new()` SHALL allocate a tempdir; all tantivy `open_write` / `atomic_write` calls SHALL delegate to the mmap directory while the wrapper records every file path written. `PuffinDirWriter::set_property(key, value)` SHALL store file-level properties to be emitted in the final `PuffinMeta.properties` map. `PuffinDirWriter::to_puffin_bytes()` SHALL: (1) iterate every recorded file path whose extension is in the allow-list `{ "term", "idx", "pos", "store", "fast", "fieldnorm", "del", "json", "lock" }`; (2) read each file's bytes via the mmap directory; (3) append it as a blob with `blob_type = O2TtvV1` and `properties["blob_tag"] = <relative path>`; (4) append a footer-cache blob with `blob_type = O2TtvFooterV1` containing tantivy segment metadata serialized by `build_footer_cache`; (5) finish and return the full puffin bytes.

#### Scenario: Tantivy writes go to mmap directory transparently
- **WHEN** tantivy's `IndexWriter::commit()` triggers writes to `.term`, `.idx`, `.fast`, `meta.json`
- **THEN** every file lands under the tempdir; `PuffinDirWriter.file_paths` records all four paths; no puffin bytes are produced until `to_puffin_bytes()` is called

#### Scenario: Allow-list filters non-tantivy files
- **WHEN** `to_puffin_bytes()` is invoked and the tempdir contains an unrelated file `noise.tmp`
- **THEN** the produced puffin file contains no blob whose `blob_tag` is `noise.tmp`; only allow-listed extensions are included

#### Scenario: Footer-cache blob is the last blob
- **WHEN** `to_puffin_bytes()` succeeds
- **THEN** the produced `PuffinMeta.blobs` ends with exactly one entry whose `blob_type = O2TtvFooterV1` and `blob_tag = "footer_cache"`

### Requirement: Parquet→Tantivy Sidecar Key Mapping

The system SHALL provide a pure function `convert_parquet_file_name_to_tantivy_file(parquet_key: &str) -> Option<String>` that maps a molesignal parquet object key `{org}/{stream_type}/{stream}/{YYYY-MM-DD}/{ksuid}.parquet` to its companion puffin sidecar key `files/{org}/index/{stream}_{stream_type}/{YYYY}/{MM}/{DD}/00/{ksuid}.ttv`. The function SHALL return `None` for inputs that do not match the expected 5-segment layout, that lack a `.parquet` extension, or whose date segment is not a parseable `YYYY-MM-DD`. The hour segment SHALL always be the literal `00` until daily-only partitioning is replaced.

#### Scenario: Standard parquet key maps to canonical sidecar
- **WHEN** input is `orgA/logs/log_app/2026-01-15/abc123.parquet`
- **THEN** output is `Some("files/orgA/index/log_app_logs/2026/01/15/00/abc123.ttv")`

#### Scenario: Stream type traces is reflected in the sidecar stream segment
- **WHEN** input is `orgA/traces/svc/2026-03-04/xyz.parquet`
- **THEN** output is `Some("files/orgA/index/svc_traces/2026/03/04/00/xyz.ttv")`

#### Scenario: Wrong segment count returns None
- **WHEN** input is `orgA/logs/log_app/2026-01-15.parquet` (4 segments) or `orgA/logs/log_app/2026-01-15/abc/extra.parquet` (6 segments)
- **THEN** the function returns `None`

#### Scenario: Wrong extension returns None
- **WHEN** input ends with `.tar.zst` or `.ttv` rather than `.parquet`
- **THEN** the function returns `None`

#### Scenario: Malformed date returns None
- **WHEN** the date segment is `26-1-15` or `2026/01/15` or `not-a-date`
- **THEN** the function returns `None`

### Requirement: Puffin Reader Metrics

The `tantivy_utils` crate SHALL export Prometheus metrics observing puffin read behaviour: `tantivy_puffin_footer_bytes_read_total` (Counter; bytes read for footer parse), `tantivy_puffin_blob_range_reads_total` (Counter; one per sub-range get_range), `tantivy_puffin_directory_open_total` (Counter; one per `PuffinDirReader::from_object_store` call), `tantivy_puffin_directory_open_seconds` (Histogram; footer-fetch+parse latency). These metrics SHALL be in addition to existing `tantivy_pruned_files_total` and `tantivy_missing_archive_total`.

#### Scenario: Footer fetch and blob reads counted distinctly
- **WHEN** a single `PuffinDirReader::from_object_store` call is followed by 5 distinct tantivy reads triggering 5 sub-range get_range
- **THEN** `tantivy_puffin_directory_open_total` increments by 1, `tantivy_puffin_footer_bytes_read_total` increments by the actual footer bytes read (≥ 12), `tantivy_puffin_blob_range_reads_total` increments by 5

#### Scenario: Histogram captures open latency
- **WHEN** an open completes
- **THEN** `tantivy_puffin_directory_open_seconds` observes one sample equal to the wall-clock time from call start to handle return
