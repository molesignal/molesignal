## ADDED Requirements

### Requirement: Role-based Startup

`crates/bootstrap` SHALL read `[node].roles` from settings and, for each role, start the matching subsystem (`http_server` for `standalone`, `router::run`, `ingester::run`, `querier::run`, `compactor::run`, `alert_manager::run`).

#### Scenario: Standalone runs every subsystem in-process
- **WHEN** `roles = ["standalone"]`
- **THEN** the HTTP server, ingester, querier, compactor, and alert_manager all run in the same process, sharing the same `AppState`

#### Scenario: Mixed roles per node
- **WHEN** `roles = ["ingester", "querier"]`
- **THEN** only the ingester and querier subsystems start; the HTTP API does not bind

### Requirement: Node Heartbeat

Each running role SHALL register itself in the cluster registry by emitting a `NodeHeartbeat { node_id, role, advertise_addr, ts }` over `proto.cluster.NodeService` at most every 5 seconds, and a node is considered alive if a heartbeat arrived within the last 15 seconds.

#### Scenario: Stale node is pruned
- **WHEN** a registered node misses heartbeats for >15s
- **THEN** the router stops listing it as an upstream candidate and queriers stop dispatching Arrow Flight sub-plans to it

### Requirement: Router Reverse Proxy & Rate Limit

The router role SHALL forward `/api/v1/ingest/*` to a healthy ingester and `/api/v1/query` to a healthy querier (or directly to the local subsystem when colocated), applying a token-bucket rate limit per `(org_id, route)` defined in `[router.rate_limit]`.

#### Scenario: Ingest is sharded
- **WHEN** two ingesters are healthy
- **THEN** consecutive ingest requests for the same `(org, stream)` consistently hash to the same ingester (so WAL is local) but different `(org, stream)` pairs spread across both

#### Scenario: Rate limit exceeded
- **WHEN** a client exceeds the bucket for `(org, /api/v1/ingest/*)`
- **THEN** the response is `429 Too Many Requests` with `Retry-After` header

### Requirement: Wire Assembly

`crates/bootstrap/src/wire.rs::build_state` SHALL construct a fully-populated `AppState` (every `Arc<dyn ...>` non-null), having connected to the meta store, run pending sea-orm migrations, built the object store, and instantiated each `*Service`.

#### Scenario: Bad DSN fails fast
- **WHEN** `meta_store.dsn` is unreachable
- **THEN** `main()` returns an error before any role subsystem starts

#### Scenario: Migrations run on startup
- **WHEN** the server starts against a fresh meta store
- **THEN** all pending migrations are applied before `build_state` returns
