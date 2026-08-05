## 0. 准备 / shared 层

- [x] 0.1 `crates/shared/src/license.rs`：`LicenseGate` 加 `fn features(&self) -> Vec<&'static str>` 默认实装返回 `vec![]`；`CommunityLicense::features` 显式 `vec![]`
- [x] 0.2 enterprise license crate 实装 `features()` 返回 parsed feature list（如果该 crate 在主 repo 中存在；否则只在 enterprise 仓库跟进）

## 1. `/license` 端点

- [x] 1.1 `crates/api/src/http/routes/license.rs`：新建 module，`GET /license` handler 读 `state.license` 返 `{edition, verified, expired, issued_to, features, max_ingest_bytes_per_day, expires_at_micros}`
- [x] 1.2 `routes/mod.rs` 加 `pub mod license` + `merge(license::routes())`

## 2. `/model_prices` 端点

- [x] 2.1 `crates/infra/src/persistence/repositories/model_prices.rs`：`ModelPriceRepository` 加 `delete(provider, model)` 方法 + Pg impl
- [x] 2.2 `crates/api/src/http/routes/model_prices.rs`：`GET /model_prices`（list）/ `POST /model_prices`（upsert）/ `DELETE /model_prices/{provider}/{model}`，全部 OrgAdmin+
- [x] 2.3 `routes/mod.rs` 加 `pub mod model_prices` + `merge(model_prices::routes())`

## 3. `/alerts/templates` 端点

- [x] 3.1 新建迁移 `crates/infra/src/persistence/migrations/NNNN_add_alert_templates.sql`：`alert_templates` 表 (id, org_id, name, body, format, created_at_micros, updated_at_micros, UNIQUE(org_id, name))
- [x] 3.2 `crates/infra/src/persistence/repositories/alert_templates.rs`：`AlertTemplate` struct + `AlertTemplateRepository` trait + `PgAlertTemplateRepository` impl（list/create/delete，scope by org_id）
- [x] 3.3 `repositories/mod.rs` 添加 `pub mod alert_templates`
- [x] 3.4 `crates/api/src/state.rs`：`AppState` 加 `pub alert_templates: Arc<dyn AlertTemplateRepository>`
- [x] 3.5 `crates/api/src/http/routes/alert_templates.rs`：`GET /alerts/templates` / `POST /alerts/templates` / `DELETE /alerts/templates/{id}`；format 校验（text/markdown/html）；name 冲突返 409
- [x] 3.6 `routes/mod.rs` 加 `pub mod alert_templates` + `merge(alert_templates::routes())`
- [x] 3.7 wire (bootstrap)：构造 `PgAlertTemplateRepository` 注入 AppState

## 4. `/regex_patterns` 端点

- [x] 4.1 新建迁移 `NNNN_add_regex_patterns.sql`：`regex_patterns` 表 (id, org_id, name, pattern, description, created_at_micros, updated_at_micros, UNIQUE(org_id, name))
- [x] 4.2 `crates/infra/src/persistence/repositories/regex_patterns.rs`：trait + Pg impl
- [x] 4.3 `repositories/mod.rs` 加 `pub mod regex_patterns`
- [x] 4.4 `AppState` 加 `pub regex_patterns: Arc<dyn RegexPatternRepository>`
- [x] 4.5 `crates/api/src/http/routes/regex_patterns.rs`：CRUD handler；create 时调 `regex::Regex::new` 校验，失败返 400
- [x] 4.6 `routes/mod.rs` 加 `pub mod regex_patterns` + `merge`
- [x] 4.7 wire 注入

## 5. `/ai_toolsets` 端点

- [x] 5.1 `crates/domain/src/copilot_telemetry/mod.rs`（或合适位置）：`AiToolset` struct + `AiToolsetRepository` trait
- [x] 5.2 `crates/infra/src/persistence/repositories/ai_toolsets.rs`：`EmptyAiToolsetRepository`（OSS 默认）；list 返空 vec、create/delete 返 `Err::forbidden`
- [x] 5.3 `repositories/mod.rs` 加 `pub mod ai_toolsets`
- [x] 5.4 `AppState` 加 `pub ai_toolsets: Arc<dyn AiToolsetRepository>`
- [x] 5.5 `crates/api/src/http/routes/ai_toolsets.rs`：CRUD handler，OrgAdmin+
- [x] 5.6 `routes/mod.rs` 加 `pub mod ai_toolsets` + `merge`
- [x] 5.7 wire：OSS 注入 `EmptyAiToolsetRepository`；enterprise crate 自己提供 Pg impl 替换（follow-up）

## 6. `/query/running` + cancel

- [x] 6.1 `crates/app/src/query/mod.rs`：新增 `QueryRegistry`（`parking_lot::RwLock<HashMap<QueryId, ActiveQuery>>`）+ `ActiveQuery { id, org_id, user_id, statement, started_at_micros, cancel: Arc<AtomicBool> }`
- [x] 6.2 `QueryService::execute_query` / `execute_stream_query`：进入时 register、退出时 unregister（RAII guard）；在 batch 边界检查 cancel flag
- [x] 6.3 `QueryService` 暴露 `running(&org_id_filter, only_owner: bool) -> Vec<ActiveQuerySnapshot>` 和 `cancel(query_id)` 两个 method
- [x] 6.4 `crates/api/src/http/routes/query.rs`：新增 `GET /query/running` handler，OrgAdmin+ 取自己 org，Owner 取全部
- [x] 6.5 `crates/api/src/http/routes/query.rs`：新增 `POST /query/{id}/cancel` handler，校验 caller 拥有该 query 或是 Owner，404/403 边界处理
- [x] 6.6 `routes/query.rs` 注册两个新 route

## 7. 前端去 awaitingBackend

- [x] 7.1 `web/src/api/license.ts`：新建 client `get(): Promise<LicenseSnapshot>`
- [x] 7.2 `web/src/api/regexPatterns.ts`：新建 `list/create/remove`
- [x] 7.3 `web/src/api/aiToolsets.ts`：新建 `list/create/remove`
- [x] 7.4 `web/src/api/modelPricing.ts`：新建（指向 `/model_prices`，name 与 i18n 对齐）
- [x] 7.5 `web/src/api/index.ts` 导出新增 client
- [x] 7.6 `routes/settings/License.tsx`：去掉 `EmptyState awaitingBackend`，用 `useQuery` + KvRow 展示 license snapshot
- [x] 7.7 `routes/settings/ModelPricing.tsx`：去 awaitingBackend，list + 上传 drawer
- [x] 7.8 `routes/settings/AlertTemplates.tsx`：endpoint 已存在，去掉 retry:false fallback 测试
- [x] 7.9 `routes/settings/RegexPatterns.tsx`：去 awaitingBackend，list + 创建 drawer
- [x] 7.10 `routes/settings/AiToolsets.tsx`：去 awaitingBackend，list（OSS 返空时仍渲染空 list 而非 awaitingBackend）+ 创建 drawer（OSS 时按钮禁用）
- [x] 7.11 `routes/settings/QueryManagement.tsx`：去 awaitingBackend，list + cancel 实接

## 8. 文档 + 校验

- [x] 8.1 `docs/web/sitemap-diff.md` P1 表：`license / model_pricing / alert_templates / regex_patterns / ai_toolsets / query_management` backend 列由 🚧 → 🔌
- [x] 8.2 `cargo check -p molesignal-api --no-default-features` 通过（OSS build）
- [x] 8.3 `cargo check -p molesignal-api --features enterprise` 通过
- [x] 8.4 `cargo test -p molesignal-infra` 通过（含新 migration）
- [x] 8.5 `pnpm -C web typecheck` 0
- [x] 8.6 `pnpm -C web lint` 0
- [x] 8.7 `pnpm -C web test:run` 不退化（新增改动不引入新失败；现存 keyboard controller test 失败与本次变更无关）
- [x] 8.8 `openspec validate backend-settings-endpoints --type change --strict` 通过
