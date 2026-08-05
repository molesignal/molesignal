## Why

跨越 `production-core-engine` / `feature-parity-with-openobserve` / `auth-hardening` / `web-investigation-shell` 四个已归档 change，累计有 15 套集成测试在 spec 内被列为"留 follow-up"。每条都已经有 unit test 覆盖核心逻辑，但缺端到端 testcontainer + 真实 HTTP / gRPC 流过验证。1.0 发版前必须把这层补齐，否则发生 regression 时只能靠 staging 手工冒烟。

## What Changes

实装下列 15 个集成测试（全部 `MS_RUN_IT=1` 门控、复用 `tests/common::TestServer` fixture、用 testcontainers postgres）：

- `it_service_graph.rs`：traces 入 ingest → service_graph_aggregator drain → edges insert → `/api/v1/traces/service_graph` 返预期边
- `it_anomaly_mad.rs`：注入 baseline 数据 → MAD detector run → outliers detected list
- `it_copilot_fanout.rs`：copilot 路由 cfg=enterprise + license check + handler 行为
- `it_rum_ingest.rs`：RUM session/action/error/replay 入 ingest → DB 行 / object_store 文件齐
- `it_cipher_keys.rs` 扩展：原 lib 测试已存在，补 e2e（rotate + encrypt-decrypt 同 key_id 兼容 + 老 version verify）
- `it_license_gates.rs` 扩展：current 测的是 copilot 404；补 actions / mcp / chat / marketplace / domains / fga 6 个路由 license-gate
- `it_scheduled_pipelines.rs` 扩展：现有冒烟基础上加 cron tick → SQL 跑出预期行 → 写目标 stream
- `it_connectors.rs` 扩展：现有 CRUD；补 cloudwatch / kinesis / cloudflare / heroku 4 个 connector kind 各跑一次
- `it_search_around.rs` 扩展：现有冒烟；补 before/after 边界 + fingerprint 命中 + 跨 day 边界
- `it_short_url.rs`：create → /s/<code> 302 → click_count 增 → expired → 410
- `it_annotations.rs`：CRUD + tag 过滤 + 跨 org 隔离 + dashboard / stream filter
- `it_sourcemaps.rs`：upload multipart → object_store 落文件 → translate_frame 反翻栈
- `it_log_patterns.rs`：CRUD + compile_check 拒坏 regex + first_match HTTP endpoint
- `it_search_jobs.rs`：Prefer: respond-async → 202 + job row → worker pickup → state=done → results 拿 ndjson
- `it_scheduled_reports.rs`：create report → cron tick → render → deliver to webhook (起 wiremock) → `report_deliveries` 落 row

## Capabilities

### New Capabilities
<!-- 无 -->

### Modified Capabilities
<!-- 无；本 change 仅补集成测试，不改 spec 行为 -->

## Impact

- **新增 / 扩展 15 个 `it_*.rs`**：全部在 `crates/bootstrap/tests/`。
- **测试基础设施**：`tests/common/mod.rs` 加 helper（`stream_seed_helpers` / `wiremock_setup` / `wait_until` 等），现有 fixture 几乎不动。
- **新增 dev-deps**：`wiremock = "0.6"`（HTTP mock）；`testcontainers-modules` 已有。
- **运行成本**：单条 it 测试约 5-10s（pg container startup），15 条全跑 ~3min；CI 单独 job。
- **CI**：`.github/workflows/it.yml` 增 `MS_RUN_IT=1 cargo test --workspace --tests --test-threads=1`，分 stage 跑（OSS + enterprise 两 matrix）。
- **不破坏**：所有测试 `MS_RUN_IT=1` 门控，本地 `cargo test` 默认跳过。
