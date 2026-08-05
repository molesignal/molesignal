## ADDED Requirements

### Requirement: Profile Archive Object Layout

The storage layer SHALL archive each ingested profile as a zstd-compressed pprof object at object_store key `profiles/<org_id>/<service>/<profile_type>/<yyyymmdd>/<profile_id>.pprof.zst`, and SHALL remove the archived object together with its profile metadata when retention expires.

#### Scenario: Archive key format

- **WHEN** a profile for org `o1`, service `api`, type `cpu` is archived for date 2026-06-18
- **THEN** the object key is `profiles/o1/api/cpu/20260618/<id>.pprof.zst`

#### Scenario: Retention removes blob and metadata together

- **WHEN** a profiles stream partition passes its retention window
- **THEN** both the parquet metadata rows and the archive objects they reference are deleted
