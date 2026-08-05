# MMDB Enrichment Capability

## Purpose

MaxMind GeoLite2 DB 周期下载 + IP → location 查询，并提供 VRL `geoip_lookup` builtin 用于 ingest path 与 functions runtime。

## Requirements

### Requirement: MMDB download and refresh

The system SHALL download MaxMind GeoLite2-City.mmdb on startup if `MS_MMDB_LICENSE_KEY` is set, and refresh weekly via a background task. The file lands at `<data_dir>/mmdb/GeoLite2-City.mmdb`. Missing license key SHALL log a warn but NOT block startup.

#### Scenario: Missing key does not crash

- **WHEN** the system starts without `MS_MMDB_LICENSE_KEY`
- **THEN** startup proceeds, a warn log is emitted, and `geoip_lookup` returns NULL for all IPs

### Requirement: VRL `geoip_lookup` builtin

The system SHALL register a VRL builtin function `geoip_lookup(ip: string)` that returns `{ country, region, city, latitude, longitude }` or NULL if no match.

#### Scenario: Public IP enriched in pipeline

- **WHEN** a pipeline function calls `.geo = geoip_lookup(.client_ip)` and the IP is in the MMDB
- **THEN** the event carries `geo: { country, region, city, latitude, longitude }` after pipeline execution
