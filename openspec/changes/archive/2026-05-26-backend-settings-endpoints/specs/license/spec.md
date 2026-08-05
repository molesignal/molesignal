## ADDED Requirements

### Requirement: License Read Endpoint

The system SHALL expose `GET /api/v1/license` (any authenticated user) returning a flat JSON snapshot of the active `LicenseGate`: `{ edition: "community" | "enterprise", verified: bool, expired: bool, issued_to: string, features: string[], max_ingest_bytes_per_day: number | null, expires_at_micros: number | null }`. This endpoint complements the previously-spec'd `/api/v1/system/license` (Admin-only Owner) — the new path is intentionally read-broad so any logged-in user can see the plan they're working under.

#### Scenario: Community license rendered

- **WHEN** the server is started without a license file and any user GETs `/api/v1/license`
- **THEN** the response is `{ edition: "community", verified: false, expired: false, issued_to: "community", features: [], max_ingest_bytes_per_day: null, expires_at_micros: null }`

#### Scenario: Enterprise license rendered

- **WHEN** the server is started with a valid signed license that includes `["sso", "copilot"]`
- **THEN** the response is `{ edition: "enterprise", verified: true, expired: <bool>, issued_to: "<name>", features: ["sso", "copilot"], max_ingest_bytes_per_day: <n>, expires_at_micros: <n> }`

### Requirement: LicenseGate Features Method

The `LicenseGate` trait SHALL expose `fn features(&self) -> Vec<&'static str>` (default impl returns `vec![]`) so handlers can enumerate the active feature set without knowing the full feature catalog. `CommunityLicense::features` returns `vec![]`; the enterprise `SignedLicense` implementation returns its parsed feature list.

#### Scenario: Trait default keeps OSS unchanged

- **WHEN** the OSS build compiles
- **THEN** `CommunityLicense::features()` returns an empty vec (no panic, no extra trait bound)
