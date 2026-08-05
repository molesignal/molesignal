## ADDED Requirements

### Requirement: Async File Downloader

The system SHALL expose `POST /api/v1/files/download` accepting `{ object_keys: [<key>], expires_in_secs }` and returning `{ download_url, expires_at }`. The URL SHALL be a pre-signed S3 URL when the backend is `s3`, or a temporary streaming endpoint `/api/v1/files/stream/<token>` for other backends. Required permission: `Permission::StreamRead` for the underlying stream.

#### Scenario: Pre-signed URL for S3

- **WHEN** a user POSTs `{ "object_keys": ["app/2026/05/file-xxx.parquet"], "expires_in_secs": 3600 }` on an S3-backed deployment
- **THEN** the response carries an HTTPS pre-signed URL that downloads the parquet directly from S3

#### Scenario: Streaming token for local backend

- **WHEN** the same request is made on a `local` backend deployment
- **THEN** the response carries `/api/v1/files/stream/<token>`; GET on that URL streams the file body for the duration of `expires_in_secs`

### Requirement: Org Schema Cache

The system SHALL maintain an in-memory `OrgSchemaCache` keyed by `(org_id, stream_name, stream_type)` → `Arc<Schema>` with TTL 60s + invalidation on `StreamRepository::update_schema`. Cache hits SHALL avoid the DB roundtrip on every ingest event.

#### Scenario: Schema update invalidates cache

- **WHEN** `PUT /api/v1/streams/<id>/schema` adds a column
- **THEN** subsequent ingest events for that stream see the new schema within 1 second (cache invalidation propagated)

#### Scenario: Cache hit avoids DB

- **WHEN** 10000 ingest events for the same `(org, stream)` arrive in a 60-second window
- **THEN** at most 1 DB roundtrip is made to fetch the schema; the remaining 9999 events use the cache
