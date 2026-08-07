<div align="center">

# MoleSignal

**Go from metric to trace to log without losing context.**

Self-hosted and OpenTelemetry-native, MoleSignal puts logs, metrics, and traces on one storage layer and one query engine—so a trace, its logs, and the host metrics around it are connected by design, not stitched together by hand.

[Why](#why-molesignal) · [Quick start](#quick-start) · [Features](#features) · [Architecture](#architecture) · [Status](#status) · [Chinese](README.zh-CN.md)

</div>

---

## Why MoleSignal

Today's telemetry tools force a bad trade-off:

- **Commercial SaaS** (Datadog, New Relic, Splunk) — three signals are correlated, but the bill grows linearly with traffic. A mid-size team easily pays **US$2k–10k/month** for 100 GB/day, and reducing ingest means losing visibility.
- **Open-source stacks** (Loki + Mimir + Tempo + Grafana, or ELK + Prometheus + Jaeger) — free, but **logs / metrics / traces live in three separate stores with three query languages**. The "trace ↔ log ↔ host metric" jump everyone needs during an incident has to be stitched by hand: copy a trace_id, switch tab, paste, repaste a time range, hope the clocks agree.

molesignal takes the third path: **one storage layer (Parquet on object store), one query engine (DataFusion + Arrow), one metadata layer (Postgres)** — so the three signals are correlated at the data plane, not at the dashboard plane. Self-hosted, so your bill is the S3 cost.

|  | Commercial SaaS<br>(Datadog / New Relic) | OSS stack<br>(Loki + Mimir + Tempo) | **molesignal** |
|---|---|---|---|
| 100 GB/day cost | ~US$2k+/mo (grows linearly) | infra only | **infra only** |
| Three signals — same storage | ✅ (their cloud) | ❌ 3 stores, 3 query langs | **✅ Parquet + DataFusion** |
| Cross-signal correlation | ✅ (paid) | ⚠️ manual trace_id copy-paste | **✅ native (`/web/correlation/*`)** |
| Data ownership | their cloud | self-hosted | **self-hosted** |
| Setup time | 5 min (agents) | 6 hours+ (5 components + Grafana) | **1 cmd `docker compose up`** |
| OpenTelemetry-native | yes | partial | **yes (9 ingest protocols)** |
| Real-time alerts (<1s) | yes | no (eval interval ≥ scrape interval) | **yes (`kind: realtime`)** |
| Multi-tenant out-of-box | yes (per-account) | no | **yes (planner-level org rewrite)** |

> **Status:** early. Released **NOT YET** — currently pre-1.0, looking for first contributors and design partners. See [Status](#status).

---

## Quick start

```bash
git clone https://github.com/molesignal/molesignal
cd molesignal

# 1-command sandbox (Postgres + MinIO + molesignal standalone)
docker compose -f deploy/docker/docker-compose.yaml --profile standalone up

# UI:        http://localhost:5080
# S3 admin:  http://localhost:9001  (minioadmin / minioadmin)
```

Send your first data:

```bash
# OTLP HTTP (works with OpenTelemetry Collector / SDK / Vector / Fluent Bit out of the box)
curl -X POST http://localhost:5080/api/v1/ingest/logs/app \
  -H 'content-type: application/json' \
  -H 'authorization: Bearer <jwt>' \
  -d '[{"_timestamp":1700000000000000,"level":"error","msg":"db pool exhausted","trace_id":"abc123"}]'

# Query — and notice the same trace_id ties logs to traces and to host metrics
curl -X POST http://localhost:5080/api/v1/query \
  -H 'authorization: Bearer <jwt>' \
  -H 'content-type: application/json' \
  -d '{"org_id":"<from-login>","language":"sql",
       "statement":"SELECT * FROM app WHERE trace_id = '\''abc123'\''",
       "time_range":{"start":0,"end":2000000000000000},
       "stream":{"name":"app","stream_type":"logs"}}'
```

**No data yet?** Open the UI Home page and click **Load sample data** — it ingests a
built-in cross-signal demo (logs + metrics + traces sharing trace_ids) so you can try a
`metric → trace → log` drill-down in seconds.

Full integration examples for Vector / Fluent Bit / OTel Collector / Prometheus remote_write are in [docs/integrations.md](docs/integrations.md).

---

## Features

### 🔗 Cross-signal correlation (the killer feature)

A trace, its logs, and the host's metric for the same minute share **the same storage, the same time index, the same tenant scope**. No more "copy this trace_id, switch tab, paste, hope":

- `GET /api/v1/web/correlation/{from_kind}/{to_kind}` — server-side join across signals with prefilled filters
- Service graph + RED metrics derived from traces, ready to be a topology view
- Time anchor synchronizes all panels (one click to zoom + propagate)
- Investigation stack: drill `metric → trace → log → host` and back without losing context

### 📡 Ingest (9 protocols, drop-in replacements)

| Protocol | Endpoint | Drop-in for |
|---|---|---|
| OTLP gRPC | `:5082` | OpenTelemetry SDK / Collector |
| OTLP HTTP | `POST /api/v1/{logs,metrics,traces}` | OTel HTTP exporter |
| Prometheus remote_write | `POST /api/v1/prometheus/api/v1/write` | Prometheus / VictoriaMetrics |
| Elasticsearch `_bulk` | `POST /api/v1/_bulk` | Filebeat, Vector ES sink, Logstash |
| Loki push | `POST /api/v1/loki/api/v1/push` | Promtail, Vector Loki sink |
| Kinesis Firehose | `POST /api/v1/_kinesis_firehose` | AWS Firehose |
| Cloudflare Logpush | `POST /api/v1/_cloudflare` | Cloudflare Logpush |
| Heroku log drain | `POST /api/v1/_heroku` | Heroku |
| Native HTTP JSON | `POST /api/v1/ingest/{type}/:stream` | curl / app SDK |

### 🌐 RUM & APM

- **RUM** — Datadog-compatible receiver for sessions, actions, errors, and replay; sourcemap symbolication for JavaScript and native stacks
- **APM** — service graph, RED metrics, and dependency views derived from traces; service/endpoint drill-down

### 🗃️ Storage & query — one engine for everything

- **Columnar storage** — Parquet on S3 / GCS / Azure / MinIO; Postgres for metadata
- **Tantivy inverted index** — file-level pruning at query time (typically ~99% reduction)
- **DataFusion query engine** — full SQL with joins / CTEs / window functions across logs, metrics, traces
- **PromQL subset** — `rate`, `increase`, `sum/avg/min/max/count by/without`, `histogram_quantile` ([roadmap](docs/promql_subset.md))
- **Distributed query via Arrow Flight** — coordinator shards by consistent hash, peers stream `RecordBatch` back
- **3-level cache** — `parquet_file_meta` / `parquet_meta` / `query_result` plus a parquet disk cache enabled by default (`./data/cache/parquet`, 10 GB LRU; tune or turn off via `[cache.disk_cache]`)
- **ParquetFileMeta cold-tier spillover** — partitions older than `[storage.parquet_file_meta_dump].cold_after_days` (default 30) are serialized to object storage so the main metadata table stays small; queries transparently merge hot + cold sources

### 📊 Dashboards

- Custom dashboards with chart variables, time-series / stat / table / topology panels
- Dashboard contracts for version-controlled provisioning and sync

### 🌍 Federated Search

- Cross-cluster resource sync via CloudEvents 1.0
- Dashboards, alert rules, and regex patterns shared across clusters with Lamport-versioned conflict resolution

### 🚨 Alerting

- **Three rule kinds**: `scheduled` (periodic SQL eval), `realtime` (in-ingest predicate match, fires <1s), `anomaly` (MAD + EWMA detectors; daily baseline with opt-in weekly seasonality; 0–1 score + human-readable reason)
- **Escalation policies** — multi-step with ack timeout, on-call rotations, overrides
- **Channels** — Slack, email, and webhooks: generic + Lark/Feishu/WeCom/DingTalk group robots + PagerDuty / OpsGenie / Microsoft Teams; template variables

### 🛡️ Platform

- **SSO** — OIDC / SAML / LDAP with field mapping and group-role binding
- **RBAC** — API tokens (`ms_<prefix>_<secret>`) and JWT; per-token role, expiry, last-used tracking
- **Multi-tenant** — planner-level `org_id` rewrite; cross-org data leak impossible by construction
- **Audit log** — every mutating operation recorded
- **Field-level encryption** — AES-256-GCM + cipher root key envelope; VRL `encrypt()` / `decrypt()` builtins
- **Per-org quotas** — ingest QPS / query QPS / storage cap

### 🤖 Mole Intelligence

- Natural-language chat interface over your telemetry data (SSE streaming)
- MCP server for integrating with AI assistants
- Configurable model providers, toolsets, and prompts per org
- Dashboard draft generation from conversation

### ⌨️ Keyboard-friendly web UI

- ⌘K command palette — every stream, dashboard, alert, saved view is one keystroke away
- Investigation stack (max 6 frames) — push frames as you drill; `⌘[` / `⌘]` to navigate; pin to keep context
- Every clickable action is keyboard-reachable

### 🧩 Pipeline functions (VRL + optional JS + LLM)

Functions are reusable transforms attached to a pipeline step. Three kinds are supported on the ingest hot path:

- **VRL** — always available. Compiled per `(function_id, updated_at)`, evaluated with the upstream `vrl::compiler` stdlib (`del`, `parse_json`, `to_int`, `match`, `encrypt` / `decrypt`, …).
- **JavaScript** — opt-in, built on `deno_core` (V8). Disabled by default because adding `deno_core` pushes a clean workspace build from ~1.5 min to ~5 min. Enable at compile time with `--features js-runtime` (no runtime toggle needed — the binary either has V8 or it doesn't). If the feature is off, a JS function POST returns `400 javascript runtime not enabled`, and any existing JS row reaching the pipeline fails with `IngestError { reason: "javascript runtime disabled" }`.
- **LLM** — opt-in, passes the event JSON through a configured AI provider (intelligence) and writes the model output back into a configurable field (default `_llm_eval`). Gated by the runtime config toggle:

  ```toml
  [functions]
  llm_eval_enabled = true
  ```

  When disabled, `language=llm` rows are rejected at the pipeline.

**Surface inside the isolate**

User source runs against a minimal `globalThis.molesignal` object — there is no `fetch`, no `setTimeout`, no `import`, no `npm`, no `crypto.subtle`:

```js
// Lowercase a `severity` field into a new `level` field.
molesignal.set("level", molesignal.fields.severity.toLowerCase());
// Drop a sensitive field.
molesignal.del("pw");
// Helpers available: molesignal.now(), .log(level, msg), .parse_json(s),
// .encode_json(v), .sha256(input) — pure-JS implementations bundled in the prelude.
```

**Resource limits**

- Wall-clock budget: **50 ms per event** (`IsolateHandle::terminate_execution`); on timeout the isolate is rebuilt for the next event so state cannot poison the batch.
- Heap budget: **32 MiB per isolate** (`v8::CreateParams::heap_limits` + near-heap-limit callback → terminate).
- Anything beyond a single synchronous pass is unsupported by design — no `Promise` scheduler is installed, so `await` will not actually wait.

### ☸️ Operations

- **6 stateless roles** — `router` / `ingester(SF + PVC)` / `querier` / `compactor` / `alert-manager` / `connector`; only ingester has local state (WAL, ≤ flush_interval window)
- **Single binary** — same image serves all roles, selected by `MS_NODE_ROLES`
- **Kubernetes manifests** in [deploy/k8s/](deploy/k8s/), Docker Compose with `standalone` + `multirole` profiles
- **Prometheus `/metrics`** with rich cache / object_store / ingester / compactor metrics
- **Health probes** — readiness gated by ingester WAL replay + object_store round-trip probe

---

## Architecture

```
                          ┌──────────┐
   OTel / Vector / ...  ─►│  router  │─► consistent hash(org, stream) ─► ingester(s)
                          └──────────┘                                      │
                               │                                            ▼
                               ▼                                       WAL + Arrow buffer
                       /api/v1/{ingest,query,...}                           │
                               │                                  flush → Parquet + Tantivy
                               ▼                                  upload to S3
                       ┌──────────────┐                                     │
                       │ web shell    │                                     ▼
                       │ (⌘K + stack) │             ┌────────────────────────────┐
                       └──────┬───────┘             │ ParquetFileMeta in Postgres       │
                              │ /query              │ object_store in S3/GCS/... │
                              ▼                     └────────────────────────────┘
                       ┌──────────────┐                          ▲
                       │  querier(s)  │── Arrow Flight do_get ───┘
                       └──────────────┘    (distributed scan by hash shard)
```

**The crucial point:** logs, metrics, and traces all land in the **same** Parquet files (different streams, same physical storage). A SQL query can join across them natively — no cross-store federation, no manual trace_id reconciliation.

---

---

## Status

Pre-1.0, **early**. Released YYYY-MM-DD.

| Area | State |
|---|---|
| Ingest path (WAL + buffer + flush) | ✅ working |
| 9 ingest protocols | ✅ working |
| Distributed query (Arrow Flight) | ✅ working |
| 3-level cache + disk cache | ✅ working |
| Multi-tenant planner rewrite | ✅ working |
| Real-time + scheduled + anomaly alerts | ✅ working |
| Cipher keys + audit + quotas | ✅ working |
| Cross-signal correlation API | ✅ working |
| Web shell (⌘K + investigation stack) | ✅ working |
| SSO — OIDC / SAML / LDAP | ✅ working |
| One-click sample data (first-run) | ⏳ pending |
| Production hardening | ⏳ needs real workloads |

**If you try it, please [open an issue](https://github.com/molesignal/molesignal/issues) — every report shapes the next release.** Especially valued: install friction, missing protocol fields, cross-signal correlation gaps.

---

## Building

```bash
# Open-source production artifact
BUILD_ID=local-001 cargo build --release --locked -p molesignal

# Paid build (needs access to the private feature dependencies)
BUILD_ID=local-001 cargo build --release --locked -p molesignal --features <features>

# The same binary is promoted by changing runtime deployment metadata only.
RELEASE_CHANNEL=alpha ./target/release/molesignal --config conf/config.toml
```

All deliverable artifacts use the single Cargo `release` profile. `BUILD_ID` and the Git SHA identify the artifact; runtime `RELEASE_CHANNEL` (`alpha`, `beta`, `rc`, or `stable`) identifies its deployment maturity. Promote the same binary or immutable image between channels instead of rebuilding it.

---

## Contributing

PRs welcome — start with the `tasks.md` files in `openspec/changes/*`. Conventions:

- DDD layering: don't push infra concerns into `domain/`
- Every public type has a 1-sentence doc explaining *why* it exists
- Integration tests live in `tests/*_it_*.rs`; gate behind `MS_RUN_IT=1` if they need Docker
- `cargo fmt --all` + `cargo clippy --workspace --all-targets` before pushing

Issues, RFCs, design discussions: all on GitHub. No Discord/Slack yet — we'll set one up after the first batch of users.

### Contributors

Thanks to everyone who has contributed to MoleSignal:

<a href="https://github.com/molesignal/molesignal/graphs/contributors">
  <img src="https://contrib.rocks/image?repo=molesignal/molesignal" alt="Contributors" />
</a>

---

## License

Copyright 2026 MoleSignal Authors

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
