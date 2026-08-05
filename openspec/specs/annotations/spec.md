# Annotations Capability

## Purpose

时间窗注解（deployment marker / maintenance window）CRUD，提供 dashboard / chart 叠加渲染所需的元数据 endpoint，可关联 stream / dashboard。

## Requirements

### Requirement: Annotation CRUD

The system SHALL expose `/api/v1/annotations` to create / list / update / delete time-range annotations. Each annotation carries `{ id, org_id, title, description, tags[], time_start_micros, time_end_micros, dashboard_id?, stream_name?, created_by, created_at }`. List supports query params `?from=&to=&dashboard_id=&stream=&tags=`.

#### Scenario: Annotation visible on time-series within range

- **WHEN** a user creates an annotation with `time_start_micros=1700000000000000`, `time_end_micros=1700000060000000`, `dashboard_id="dash-1"`
- **THEN** a subsequent `GET /api/v1/annotations?dashboard_id=dash-1&from=1699999000000000&to=1700001000000000` includes that annotation in the response

### Requirement: Permission and scoping

Annotation create SHALL require `Permission::DashboardEdit`. List / get SHALL require `Permission::DashboardRead`. Cross-org GET SHALL return 404 (not 403, to avoid existence leak).

#### Scenario: Cross-org annotation invisible

- **WHEN** a user in org A tries `GET /api/v1/annotations/<id>` where the annotation belongs to org B
- **THEN** the system returns 404
