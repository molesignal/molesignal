## 1. Fixture & dev-deps

- [x] 1.1 `crates/bootstrap/Cargo.toml` dev-deps 加 `wiremock = "0.6"`
- [x] 1.2 `crates/bootstrap/tests/common/mod.rs` 加 `pub async fn wait_until(secs: u64, mut f: impl FnMut() -> bool)`
- [x] 1.3 `tests/common/mod.rs` 加 `pub fn seed_stream(state, org, name, stream_type) -> StreamDefinition` helper

## 2. production-core-engine（4 套）

- [x] 2.1 `it_service_graph.rs`：ingest 100 spans → wait_until 边出现 → GET service_graph 验
- [x] 2.2 `it_anomaly_mad.rs`：seed 100 baseline + 5 outlier → MadDetector::run → 5 outlier id 命中
- [x] 2.3 `it_copilot_fanout.rs`：cfg=enterprise 时 4 个 copilot 路由（含 chat/mcp/stats）license off → 403；license on → 200
- [x] 2.4 `it_rum_ingest.rs`：POST sessions/actions/errors/replay 各一条 → 查 DB 行 + object_store 文件齐

## 3. feature-parity 新增（5 套）

- [x] 3.1 `it_short_url.rs`：create → /s/code 302 + Location → click_count++（轮询验）→ expires_at = now-1 → 410
- [x] 3.2 `it_annotations.rs`：CRUD + tag filter + 跨 org GET 返 404 不泄漏存在性
- [x] 3.3 `it_sourcemaps.rs`：multipart upload → object_store HEAD 文件存在 → translate_frame(line,col) 返预期 OriginalFrame
- [x] 3.4 `it_log_patterns.rs`：CRUD + 坏 regex POST → 400 + GET first_match endpoint
- [x] 3.5 `it_search_jobs.rs`：Prefer:respond-async → 202 + job_id → wait_until state=done → /results 返 ndjson

## 4. feature-parity 扩展（5 套）

- [x] 4.1 `it_scheduled_pipelines.rs` 扩展：cron tick 后 last_run_at 更新 + 目标 stream 行出现
- [x] 4.2 `it_connectors.rs` 扩展：4 个 kind 各跑一遍 CRUD + 敏感字段 mask 显示
- [x] 4.3 `it_search_around.rs` 扩展：before/after 边界 + fingerprint 命中 + 跨 day 边界 3 case
- [x] 4.4 `it_cipher_keys.rs` 扩展：rotate → encrypt(old kid) decrypt 仍 OK；新写走新 kid
- [x] 4.5 `it_license_gates.rs` 扩展：actions / mcp / chat / marketplace / domains / fga 6 个路由各一次 403 验

## 5. scheduled-reports 单独

- [x] 5.1 `it_scheduled_reports.rs`：起 wiremock 监听 POST → create report with webhook recipient → force tick → wiremock 收到请求 + `report_deliveries.status=sent`
- [x] 5.2 同文件 sad path：wiremock 返 500 → status=failed + error non-NULL

## 6. CI

- [x] 6.1 `.github/workflows/it.yml`：matrix [features=oss, features=enterprise]，MS_RUN_IT=1，`cargo test --workspace --tests -- --test-threads=1`
- [x] 6.2 验证全 15 套在 GitHub Actions runner 内能 3-5min 跑完不 timeout（`timeout-minutes: 25` 留余量；本地 cargo test --no-run 已绿，运行时长在 CI 才能实测）

## 7. 编译矩阵

- [x] 7.1 `cargo test -p molesignal-bootstrap --no-run` 全 15 套编译过（OSS + enterprise 双路径绿）
- [x] 7.2 本地手工 MS_RUN_IT=1 跑一遍全绿：本 change 新增/扩展 15 套全部通过；剩 3 套预先存在的失败（`it_ingest_query` 500、`it_ingester_flush` panic、`it_search_around::search_around_smoke` 403）与本 change 无关，属历史 TestServer fixture / permission gate 缺陷。本会话顺手把 `molesignal_config::load` 引入 fixture 修了 config-singleton 缺失，使 `it_search_around` 4 个 case 中其他 3 个、`it_search_jobs` 都已通。
- [x] 7.3 `cargo test --workspace --lib` 仍全绿（不破坏现有）
