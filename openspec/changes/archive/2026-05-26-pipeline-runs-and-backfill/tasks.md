## 0. 准备

- [x] 0.1 audit：调度入口实际在 `crates/infra/src/pipeline/scheduled.rs::tick_once`（当前是 stub，仅 `touch_last_run`），RAII guard 注入点为 tick_once 内的 per-pipeline 处理
- [x] 0.2 `search_jobs.request_json` 已是 `serde_json::Value`（`crates/infra/src/persistence/repositories/search_jobs.rs:47`），可直接塞 `pipeline_id` 字段

## 1. `pipeline_runs` 表 + repo

- [x] 1.1 新建迁移 `crates/infra/migrations/20260701000002_pipeline_runs.sql`：`pipeline_runs` 表 + `(pipeline_id, started_at_micros DESC)` 索引
- [x] 1.2 `crates/infra/src/persistence/repositories/pipeline_runs.rs`：`PipelineRun` struct + `PipelineRunRepository` trait + Pg impl（`record_start / record_finish / list`）
- [x] 1.3 `repositories/mod.rs` 加 `pub mod pipeline_runs`
- [x] 1.4 `crates/api/src/state.rs`：`AppState` 加 `pub pipeline_runs: Arc<dyn PipelineRunRepository>`
- [x] 1.5 wire（`bootstrap/src/wire.rs`）注入 `PgPipelineRunRepository`

## 2. Scheduler 写入边界

- [x] 2.1 `crates/infra/src/pipeline/scheduled.rs::tick_once`：进入 `record_start(...)`，退出 `record_finish(...)`；新增 `with_runs(...)` 构造函数让 wire 把 `runs` repo 注入（runner 当前未在 bootstrap 实例化，将来调度器接入时用 `with_runs`）
- [x] 2.2 `map_exec_result` helper：`Err::Cancelled(_)` → `cancelled`；其他 `Err` → `failed`；`Ok` → `succeeded`
- [x] 2.3 单元测试：`map_exec_result_success / _failure / _cancelled` 3 个新 test pass（`cargo test -p molesignal-infra pipeline::scheduled`）

## 3. `GET /scheduled_pipelines/{id}/runs`

- [x] 3.1 `crates/api/src/http/routes/scheduled_pipelines.rs`：新增 `list_runs` handler，`StreamRead` permission，分页 `limit` / `before_micros`
- [x] 3.2 cross-org 校验：`scheduled_pipelines.get(org_id, id)` 任意失败统一映射为 `Error::not_found("pipeline not found")`
- [x] 3.3 路由表注册 `GET /scheduled_pipelines/{id}/runs`

## 4. `POST /scheduled_pipelines/{id}/backfill`

- [x] 4.1 `submit_backfill` handler：解 body `{start_micros, end_micros}`，`OrgAdmin+`
- [x] 4.2 校验 `end > start` 且 `end - start <= 31 days`（`MAX_BACKFILL_WINDOW_MICROS = 31 * 24 * 3600 * 1_000_000`），越界返 400 + message
- [x] 4.3 load pipeline → 合成 `QueryRequest`（`SELECT * FROM <source_stream>` + 用户窗口）→ `search_jobs.create(...)`，`request_json` 注入 `pipeline_id` + `backfill_window_micros`
- [x] 4.4 返 `202 Accepted` + `{job_id, monitor: "/api/v1/query/jobs/<id>"}`
- [x] 4.5 路由表注册 `POST /scheduled_pipelines/{id}/backfill`

## 5. Frontend wire-up

- [x] 5.1 `web/src/api/pipelineRuns.ts`：`list(pipelineId, before_micros?, limit?)` + `submitBackfill(pipelineId, {start_micros, end_micros})`
- [x] 5.2 `web/src/api/index.ts` 导出 `pipelineRuns`
- [x] 5.3 `routes/pipelines/History.tsx`：去 `EmptyState awaitingBackend`，DataTable 渲染最近 50 条 run（含 state / duration / scanned_rows / error），5 秒自动轮询
- [x] 5.4 `routes/pipelines/Backfill.tsx`：去 `awaitingBackend`，日期范围 picker + Submit；成功后显示 monitor 链接到 `/query/jobs/<id>`

## 6. 文档 + 校验

- [x] 6.1 `docs/web/sitemap-diff.md` 中 pipeline history / backfill 两行 🚧 → 🔌
- [x] 6.2 `cargo check -p molesignal-api --no-default-features` 通过
- [x] 6.3 `cargo check -p molesignal-api --features enterprise` 通过
- [x] 6.4 `cargo test -p molesignal-infra` 通过（146 unit tests + new migration）
- [~] 6.5 `cargo test -p molesignal-app` 全通过；scheduler write-path 单测放在 `crates/infra/src/pipeline/scheduled.rs::tests`（map_exec_result 3 cases），不在 app crate
- [x] 6.6 `pnpm -C web typecheck` 0
- [x] 6.7 `pnpm -C web lint` 0
- [x] 6.8 `pnpm -C web test:run` 不退化（47/49 pass；同 keyboard controller 2 个 pre-existing failures 无关）
- [x] 6.9 `openspec validate pipeline-runs-and-backfill --type change --strict` 通过

## 7. Follow-up（不在本 change 范围）

- [ ] 7.1 `pipeline_runs` 保留策略 / 自动 prune（待 ops 反馈）
- [ ] 7.2 Backfill 窗口上限提升 config（如果 31 天频繁不够再加）
- [ ] 7.3 Pipeline runs 失败告警 hook（用现有 alerting 走）
