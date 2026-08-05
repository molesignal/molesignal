# Dashboard Capability

## Purpose

Folder/Dashboard CRUD（含分页、过滤、嵌套层级）、版本递增与作者审计、native + Grafana 仪表盘 JSON 的无损导入和导出（保留未建模字段）。

## Requirements

### Requirement: Dashboard CRUD

The system SHALL expose `GET/POST/PUT/DELETE /api/v1/dashboards` and `GET/PUT/DELETE /api/v1/dashboards/:id`, backed by `DashboardRepository`. CRUD SHALL be scoped to the caller's org and enforce `Permission::DashboardEdit` for write operations.

#### Scenario: Update preserves uid
- **WHEN** a PUT changes the panel list of an existing dashboard
- **THEN** the row's `uid` is unchanged, `updated_at` is set to `now`, and the response is `200 OK` with the new model

#### Scenario: Delete is idempotent on missing
- **WHEN** a DELETE targets a non-existent or already-deleted dashboard
- **THEN** the response is `404 Not Found` (never silently 200)

#### Scenario: Cross-org PUT rejected
- **WHEN** a user from `orgA` PUTs a dashboard owned by `orgB`
- **THEN** the response is `404 Not Found` with `{ "error": "dashboard not found" }` (no existence enumeration)

### Requirement: Folder CRUD

The system SHALL expose `GET/POST /api/v1/folders` and `GET/PUT/DELETE /api/v1/folders/:id` backed by `FolderRepository`. A `Folder` carries `{ id, org_id, name, parent_id?, created_at, updated_at }`. Deleting a folder SHALL refuse (`409 Conflict`) when it still contains dashboards or child folders.

#### Scenario: Create root folder
- **WHEN** an Editor POSTs `{ "name": "ops" }` with no `parent_id`
- **THEN** the response is `201 Created` with `{ id, name: "ops", parent_id: null, org_id, ... }`

#### Scenario: Delete folder with dashboards rejected
- **WHEN** a DELETE targets a folder that still has one dashboard
- **THEN** the response is `409 Conflict` with `{ "error": "folder not empty: 1 dashboard, 0 subfolders" }`

#### Scenario: Move folder via PUT parent_id
- **WHEN** a PUT changes `parent_id`
- **THEN** the row updates atomically; cycles (folder set as ancestor of itself) are rejected `400 Bad Request`

### Requirement: Dashboard Native and Grafana Create

The system SHALL accept dashboard creation via either path: `POST /api/v1/dashboards` with `{ "source": "native", "folder_id", "payload": <DashboardModel> }` for native creation, or `{ "source": "grafana", "folder_id", "payload": <grafana json> }` for Grafana import. Both produce the same `Dashboard` row.

#### Scenario: Native create accepted
- **WHEN** a POST has `source = "native"` and a `DashboardModel` payload missing the `uid` field
- **THEN** the server assigns a `uid` (KSUID) and stores the model; response `201 Created` with the full stored model

#### Scenario: Grafana import preserves unknown fields
- **WHEN** a POST has `source = "grafana"` and the payload includes `weirdCustomKey` at top level and `extraVendorKey` inside a panel
- **THEN** the stored row's serialized model contains both fields and a subsequent GET returns them verbatim

### Requirement: Dashboard List Pagination and Filter

`GET /api/v1/folders/:id/dashboards` and `GET /api/v1/dashboards` SHALL accept `?page=<n>&page_size=<m>&filter=<substr>&tag=<tag>` query parameters; the response shape is `{ items: Vec<DashboardSummary>, total: u64, page, page_size }`. `page_size` defaults to 50 and caps at 200.

#### Scenario: Page beyond range returns empty items
- **WHEN** `total = 30` and the request is `?page=10&page_size=10`
- **THEN** the response is `{ items: [], total: 30, page: 10, page_size: 10 }` with status `200 OK`

#### Scenario: Filter narrows results
- **WHEN** `?filter=cpu` is sent and 3 of 50 dashboards have names containing `cpu`
- **THEN** `items.len() = 3`, `total = 3`
