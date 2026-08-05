## 1. Contracts And Capacity Gate

- [x] 1.1 Define the target service/Transaction/dependency cardinality and ingest rate, run a focused PostgreSQL-vs-internal-stream spike, and record the accepted storage choice, histogram boundaries, default limits, retention, minimum version sample count and performance budgets in `design.md`.
- [x] 1.2 Create responsibility-based `src/domain/apm/`, `src/app/apm/`, `src/infra/apm/` and `src/api/http/routes/apm/` module directories without adding production files over 500 lines.
- [x] 1.3 Define domain types for service identity, APM span facts, Transaction/dependency/error identities, histogram buckets, owner snapshots, rollups, exemplars, data-quality gaps and query cursors.
- [x] 1.4 Add validated `[apm]` configuration for queue/flush bounds, cardinality limits, histogram schema, late grace, hot/rollup retention and version-comparison thresholds, without an enable or kill switch.
- [x] 1.5 Add unit tests fixing fallback service/environment semantics, protocol error classification and serialization compatibility for persisted APM types.

## 2. Persistence And Idempotent Rollups

- [x] 2.1 Add and explicitly register PostgreSQL migrations for the service catalog, version observations, service/Transaction/dependency/error owner buckets, error groups/samples, projection gaps/start marker and hourly rollups.
- [x] 2.2 Add database constraints, tenant-leading indexes, time partition/retention indexes and foreign-key cleanup so every APM read/write is organization scoped.
- [x] 2.3 Implement APM repository ports and PostgreSQL adapters for catalog/version upsert, conditional owner snapshot replacement by `snapshot_seq`, bounded error samples and projection-gap recording.
- [x] 2.4 Implement repository query methods that merge owner histogram/count snapshots and choose minute or hourly resolution without combining incompatible histogram schema versions.
- [x] 2.5 Implement an idempotent minute-to-hour rollup and retention worker that transactionally merges counts/histograms, advances completion metadata and removes expired hot/rollup rows.
- [x] 2.6 Add migration/repository integration tests for ambiguous retry idempotency, concurrent owner snapshots, rollup equivalence, retention, organization deletion and cross-tenant isolation.

## 3. Pre-Sampling Projection Pipeline

- [x] 3.1 Implement privacy-safe `CanonicalSpan` to compact `ApmSpanFact` extraction, including Resource identity, Span-kind fallback, protocol status, Transaction naming, dependency classification and bounded exception normalization.
- [x] 3.2 Implement the fixed mergeable latency histogram and tests proving cross-bucket P50/P95/P99 are computed from merged counts rather than maximum per-bucket percentiles.
- [x] 3.3 Implement the windowed cardinality limiter with service rejection, Transaction/dependency `__other__`, version/instance detail suppression and error overflow semantics plus reason metrics.
- [x] 3.4 Implement the bounded in-memory projector aggregator for service, Transaction, dependency and error minute buckets, service catalog/version observations and trace exemplars.
- [x] 3.5 Implement the non-blocking projector queue, periodic absolute owner snapshot flush, late-grace handling, gap ledger and bounded shutdown drain.
- [x] 3.6 Integrate fact projection into the trace candidate owner using `CandidateDisposition`, projecting only unique `Accepted`, `LateKept` and `LateDropped` candidates before sampling fan-out.
- [x] 3.7 Wire projector/repository/rollup lifecycle through bootstrap roles and shutdown so relevant roles start APM automatically while roles that own neither Trace candidates nor rollups do not allocate workers.
- [x] 3.8 Publish APM accepted/drop/duplicate-skip/late/overflow/queue/flush/rollup/lag metrics and detailed degraded health without changing default readiness.
- [x] 3.9 Add tests proving sampled-out Traces still contribute, OTLP retries/conflicts do not double count, a request with multiple CLIENT children counts one service request, and projection failure never backpressures Trace ingest.
- [x] 3.10 Add sanitizer/privacy tests proving forbidden headers, URL/query values, SQL text/parameters and masked organization fields never reach facts, fingerprints, persistence, APIs or diagnostic logs.

## 4. APM Query Use Cases And HTTP API

- [x] 4.1 Implement a shared APM query context that validates bounded ranges, namespace/service/environment/version filters, resolution, stable sort and cursor pagination.
- [x] 4.2 Implement overview and service-catalog queries with RED summaries/trends, health counts, instrumentation metadata, recent versions and standalone services.
- [x] 4.3 Implement service-detail composition so KPIs, trends, Transactions, dependencies, errors, versions and exemplars use one consistent query context.
- [x] 4.4 Implement Transaction and dependency ranking/filter queries with request/error counts, merged percentiles, total time and safe cross-signal filter handles.
- [x] 4.5 Implement backend error list/detail queries with trends, affected Transactions/versions, sanitized representative stack/message and explicit Trace availability.
- [x] 4.6 Implement version comparison with baseline/candidate sample counts, absolute/relative RED deltas, top regressed Transactions/errors and insufficient-data semantics.
- [x] 4.7 Add `/api/v1/apm/{overview,services,transactions,dependencies,errors,versions/compare,health}` routes with `streams.query`/`sys.telemetry.read` permission handling and 404 cross-org hiding.
- [x] 4.8 Include `resolution`, `projection_started_at`, `last_complete_bucket_at`, partial gaps and overflow dimensions consistently in APM responses.
- [x] 4.9 Update OpenAPI documentation and add handler/application tests for filters, cursors, invalid ranges, system scope, missing entities, partial windows and cross-organization access.

## 5. Web APM Foundation And Core Pages

- [x] 5.1 Add `web/src/api/apm.ts` response types and clients plus stable query-key builders covering time, organization, service, environment, version, sort and cursor.
- [x] 5.2 Create `web/src/routes/apm/` with responsibility-based files for layout, shared filter URL model, data-quality notice, formatting and page modules; keep each production file under 500 lines.
- [x] 5.3 Register canonical `/apm/*` routes, `/apm` redirect, route metadata, access rules, breadcrumbs and APM English/Chinese i18n namespaces.
- [x] 5.4 Implement APM Overview with RED KPIs/trends, service health, highest-impact services, top errors/dependency regressions and recent versions using only backend APM aggregates.
- [x] 5.5 Implement the service catalog with search, environment/version filters, instrumentation health, RED sorting and standalone-service handling.
- [x] 5.6 Implement the service workbench Overview and section navigation with consistent scope, RED trends, exemplars and related Logs/Metrics/Traces/Profiles links.
- [x] 5.7 Implement the Transaction explorer with ranking/sorting, percentile columns and filtered Trace drilldown.
- [x] 5.8 Implement the Dependency explorer with category filters, safe table/topology views and caller-service drilldown.
- [x] 5.9 Implement backend Error list/detail pages with trend/impact, sanitized stack presentation and only valid Trace/Log links.
- [x] 5.10 Implement Version Compare with baseline/candidate selectors, sample counts, neutral insufficient-data state and top regression breakdown.
- [x] 5.11 Implement loading, activation-empty, filtered-empty, permission, error, stale and partial-data states for every APM route using shared product-state primitives.

## 6. RUM Integration And Navigation Migration

- [x] 6.1 Make the existing RUM layout/routes base-path aware and mount the same components under `/apm/user-experience/*` without duplicating RUM API or page implementations.
- [x] 6.2 Add compatibility handling for every `/rum/*` path and preserve suffixes, path parameters and query strings in redirects/aliases.
- [x] 6.3 Redirect `/services` and `/services/:service` to canonical APM service routes while preserving query state; keep `/traces`, `/profiles`, `/logs` and `/metrics` canonical.
- [x] 6.4 Replace the Sidebar RUM entry with APM, update product IA/access, command palette, keyboard bindings, active-route matching and localized navigation copy.
- [x] 6.5 Update RUM/APM breadcrumbs, Source Map flows, reports, intelligence capability labels and documentation so internal `rum` identifiers remain stable while user-facing hierarchy says APM → User Experience.
- [x] 6.6 Add route-compatibility tests for all legacy RUM/service deep links and verify RUM-to-Trace context remains intact.

## 7. Verification And Rollout Readiness

- [x] 7.1 Add deterministic backend fixtures covering HTTP, RPC, messaging, database, external dependency, backend exception, multiple versions, standalone service, duplicate, late and sampled-out Trace cases.
- [x] 7.2 Add end-to-end ingest-to-APM tests for OTLP HTTP and gRPC, candidate-owner routing, minute flush, API query, rollup and tenant isolation.
- [x] 7.3 Add frontend unit tests for filter URL round-trip, query keys, data-quality states, version comparison semantics and cross-signal links.
- [x] 7.4 Add Playwright coverage for APM overview/service/Transaction/dependency/error/version/RUM routes, legacy redirects, keyboard navigation, both themes and axe critical=0.
- [x] 7.5 Run the agreed projection/storage/query load benchmarks; verify hot-path and API budgets, cardinality bounds, PostgreSQL row growth and rollup behavior, then record the results and finalized defaults.
- [x] 7.6 Run one consolidated final Rust verification round after all backend changes, then run web lint, TypeScript, unit and targeted Playwright checks without redundant build/check passes.
- [x] 7.7 Run `openspec validate add-application-performance-monitoring --strict`, update API/navigation documentation and capture the staged rollout and binary rollback runbook.

## 8. APM And RUM Information Architecture Separation

- [x] 8.1 Revise the proposal, design and web requirements so APM and RUM are independent top-level products connected through Trace, service, environment, version and session context.
- [x] 8.2 Make `/rum/*` canonical, redirect legacy `/apm/user-experience/*` paths without losing suffix/query/hash, and move Source Maps to `/rum/settings/*`.
- [x] 8.3 Replace APM's User Experience and Version Compare navigation with Traces and Deployments, and add service-level Overview/Transactions/Traces/Dependencies/Errors/Runtime/Deployments navigation.
- [x] 8.4 Implement the RUM Overview/Applications/Sessions/Pages/Errors/Performance/Session Replay navigation, Web-focused overview metrics and SDK activation empty state.
- [x] 8.5 Split APM/RUM route metadata and access rules, update localized navigation/breadcrumbs/keyboard links, and synchronize design/navigation documentation.
- [x] 8.6 Update unit and Playwright coverage for canonical routes, compatibility redirects, independent navigation, Source Maps settings and RUM activation guidance.
- [x] 8.7 Run web formatting, lint, TypeScript, unit and targeted Playwright checks, then validate the OpenSpec change in strict mode.

## 9. Prometheus-Native Exemplars

- [x] 9.1 Extend the remote_write v1 wire subset with `TimeSeries.exemplars` and validate bounded labels, duplicate names, finite values and reserved storage-field collisions before persistence.
- [x] 9.2 Persist Exemplars as isolated sidecar rows in the owning metric stream without a PromQL `value`, preserving series labels, Exemplar labels/value and millisecond-to-microsecond timestamp semantics.
- [x] 9.3 Add PromQL selector extraction, matcher filtering, retry deduplication and a bounded Prometheus-compatible GET/POST `query_exemplars` API.
- [x] 9.4 Hide internal Exemplar storage fields from the metric catalog and keep ordinary PromQL sample evaluation unchanged.
- [x] 9.5 Render Exemplar markers on the Metrics time range and deep-link retained `trace_id` values to Trace detail.
- [x] 9.6 Add ingest/query/UI regression coverage, update OpenAPI and run the consolidated backend/frontend verification.
