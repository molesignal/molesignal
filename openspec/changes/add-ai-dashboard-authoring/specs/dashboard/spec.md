## MODIFIED Requirements

### Requirement: Dashboard CRUD

The system SHALL expose `GET/POST/PUT/DELETE /api/v1/dashboards` and `GET/PUT/DELETE /api/v1/dashboards/:id`, backed by `DashboardRepository`. CRUD SHALL be scoped to the caller's org and enforce `Permission::DashboardEdit` for write operations. Every native Dashboard model accepted by create or update SHALL pass the same canonical versioned JSON Schema and server-side semantic validator before repository mutation; failed validation SHALL leave the stored model and version unchanged.

#### Scenario: Update preserves uid
- **WHEN** a PUT changes the panel list of an existing dashboard
- **THEN** the row's `uid` is unchanged, `updated_at` is set to `now`, and the response is `200 OK` with the new model

#### Scenario: Delete is idempotent on missing
- **WHEN** a DELETE targets a non-existent or already-deleted dashboard
- **THEN** the response is `404 Not Found` (never silently 200)

#### Scenario: Cross-org PUT rejected
- **WHEN** a user from `orgA` PUTs a dashboard owned by `orgB`
- **THEN** the response is `404 Not Found` with `{ "error": "dashboard not found" }` (no existence enumeration)

#### Scenario: Nested invalid model is rejected consistently
- **WHEN** create or update receives a current-version native model containing a duplicate element ID, out-of-grid position, unsupported visualization, or invalid typed panel query
- **THEN** the API returns a structured validation error and performs no repository mutation

### Requirement: Dashboard Native and Grafana Create

The system SHALL accept dashboard creation via either path: `POST /api/v1/dashboards` with `{ "source": "native", "folder_id", "payload": <DashboardModel> }` for native creation, or `{ "source": "grafana", "folder_id", "payload": <grafana json> }` for Grafana import. Both produce the same `Dashboard` row. Native models and models emitted by the Dashboard authoring compiler SHALL validate against the current canonical Dashboard contract. Grafana imports SHALL preserve unknown vendor fields while still satisfying normalized structural and security invariants required for safe rendering and querying.

#### Scenario: Native create accepted
- **WHEN** a POST has `source = "native"` and a valid `DashboardModel` payload missing the `uid` field
- **THEN** the server assigns a `uid` (KSUID), validates and stores the model, and responds `201 Created` with the full stored model

#### Scenario: Grafana import preserves unknown fields
- **WHEN** a POST has `source = "grafana"` and the payload includes `weirdCustomKey` at top level and `extraVendorKey` inside a panel
- **THEN** the stored row's serialized model contains both fields and a subsequent GET returns them verbatim

#### Scenario: Authoring compiler output uses normal create invariants
- **WHEN** a confirmed AI Dashboard operation supplies a compiler-produced native model
- **THEN** creation passes through the same Dashboard application service, canonical validation, folder ownership, versioning, author audit, quota, and federation behavior as an interactive native create

