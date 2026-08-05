## ADDED Requirements

### Requirement: Cluster Node Registry Table

The system SHALL persist active node membership in a `cluster_nodes` table with columns `(node_id TEXT PK, role TEXT, advertise_addr TEXT, started_at TIMESTAMPTZ, last_heartbeat_at TIMESTAMPTZ)`. Reads filter `last_heartbeat_at >= now() - interval '15 seconds'`.

#### Scenario: Fresh node appears after first heartbeat
- **WHEN** a new ingester node sends its first `NodeService::Heartbeat`
- **THEN** a row is inserted (or updated via UPSERT) and `cluster_registry.list_role(Ingester)` immediately returns the new node

#### Scenario: Stale node is not returned
- **WHEN** a node's `last_heartbeat_at` is older than 15 seconds
- **THEN** `list_role` excludes it from results but the row remains until a sweeper task (every 60s) deletes rows older than 5 minutes

## MODIFIED Requirements

### Requirement: Node Heartbeat

Each running role SHALL register itself in the cluster registry by emitting a `NodeHeartbeat { node_id, role, advertise_addr, ts }` over `proto.cluster.NodeService` every 5 seconds, persisting to the `cluster_nodes` table via the `NodeService` server's local handler. A node is considered alive if a heartbeat arrived within the last 15 seconds.

#### Scenario: Stale node is pruned
- **WHEN** a registered node misses heartbeats for >15s
- **THEN** the router stops listing it as an upstream candidate and queriers stop dispatching Arrow Flight sub-plans to it

#### Scenario: Heartbeat in standalone mode skips network
- **WHEN** `roles = ["standalone"]` and the heartbeat task fires
- **THEN** the call is delegated directly to the local `cluster_nodes` repository without going through gRPC

#### Scenario: Heartbeat carries advertise_addr
- **WHEN** `[cluster].advertise_addr` is set to `10.0.0.5:5082`
- **THEN** that address appears in the `advertise_addr` column and is what other roles dial when forwarding RPCs

### Requirement: Router Reverse Proxy & Rate Limit

The router role SHALL forward `/api/v1/ingest/*` to a healthy ingester (selected via consistent hashing of `(org_id, stream)`) and `/api/v1/query` to a healthy querier (round-robin), applying a token-bucket rate limit (via `governor` crate) per `(org_id, route_class)` defined in `[router.rate_limit]`. Bodies SHALL be proxied via streaming (`tokio::io::copy_bidirectional` or `reqwest::Response::bytes_stream`) without full buffering.

#### Scenario: Ingest is sharded
- **WHEN** two ingesters are healthy
- **THEN** consecutive ingest requests for the same `(org, stream)` consistently hash to the same ingester (so WAL is local) but different `(org, stream)` pairs spread across both

#### Scenario: Rate limit exceeded
- **WHEN** a client exceeds the bucket for `(org, /api/v1/ingest/*)`
- **THEN** the response is `429 Too Many Requests` with `Retry-After` header equal to the cell-refresh seconds and `router_rate_limited_total{org,route}` increments

#### Scenario: No healthy ingester
- **WHEN** all ingesters' `last_heartbeat_at` is older than 15s
- **THEN** the router returns `503 Service Unavailable` with `{ "error": "no healthy ingesters" }`

#### Scenario: Streaming body avoids buffering
- **WHEN** an ingest request body is 50 MiB
- **THEN** the router's resident memory increase during proxying stays bounded (well under 50 MiB) because chunks are forwarded as they arrive

### Requirement: Wire Assembly

`crates/bootstrap/src/wire.rs::build_state` SHALL construct a fully-populated `AppState` (every `Arc<dyn ...>` non-null), having connected to the meta store, run pending sqlx migrations, built the object store, built every cache from `caching::CachingSettings`, constructed the cluster registry repository, and instantiated each `*Service`. After `build_state`, the bootstrap SHALL start exactly one `HeartbeatTask` per declared role.

#### Scenario: Bad DSN fails fast
- **WHEN** `meta_store.dsn` is unreachable
- **THEN** `main()` returns an error before any role subsystem starts

#### Scenario: Migrations run on startup
- **WHEN** the server starts against a fresh meta store
- **THEN** all pending migrations are applied before `build_state` returns

#### Scenario: Heartbeats start after wire
- **WHEN** `[node].roles = ["ingester", "querier"]`
- **THEN** the bootstrap starts two heartbeat tasks (one per role) immediately after `build_state` resolves, and both rows appear in `cluster_nodes` within 5 seconds
