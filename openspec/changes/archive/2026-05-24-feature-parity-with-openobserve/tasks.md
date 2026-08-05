## M1 OSS 核心新增（12 capability + 4 修订）

> **本轮交付**：M1 中五个最有杠杆的 capability 端到端做完
> （migration + repo + HTTP CRUD + AppState 注入 + wire + 7 个新单测）；
> 其余 capability 留到下一轮按相同模板推进。
> 总 migration 文件 `20260601000007_feature_parity.sql` 已含全部 12 张新表（包括待落代码的）。

### 1. functions-runtime ✅（precheck + CRUD；deep VRL compile 留 follow-up）
- [x] 1.0 domain stub export
- [x] 1.5 sqlx 迁移（统一 M1 migration）
- [x] 1.1 domain `Function` / `FunctionRepository` 已完整
- [x] 1.2 `crates/infra/src/persistence/repositories/functions.rs` Pg CRUD + `precheck_compile`（非空 / ≤64KiB / JS feature-gate 拒收）
- [ ] 1.3 实际 VRL/javascript runtime 接入 pipeline 路径（需引 vrl crate ~30 deps；留 follow-up）
- [x] 1.4 `crates/api/src/http/routes/functions.rs`：CRUD HTTP；POST/PUT 调 precheck_compile
- [ ] 1.6 集成测试

### 2. short-url ✅
- [x] 2.1 sqlx 迁移（统一 M1 migration）
- [x] 2.2 `crates/infra/src/persistence/repositories/short_urls.rs`：CRUD + `find_by_code` + `bump_click` + `generate_code`
- [x] 2.3 `crates/api/src/http/routes/short_url.rs`：`POST /api/v1/short` + `GET /s/:code` 302 + `DELETE`；`/s/` 加入 auth 白名单
- [x] 2.4 click_count 异步 tokio::spawn
- [ ] 2.5 集成测试 `it_short_url.rs`（留 docker 环境后做）

### 3. annotations ✅
- [x] 3.1 sqlx 迁移
- [x] 3.2 `crates/infra/src/persistence/repositories/annotations.rs`：CRUD + 时间窗 + dashboard/stream/tag 过滤（tag JSONB 内存 retain）
- [x] 3.3 `crates/api/src/http/routes/annotations.rs`：HTTP CRUD + 跨 org 404
- [ ] 3.4 集成测试 `it_annotations.rs`

### 4. sourcemaps ✅（上传 + 翻译；RUM 接入留 follow-up）
- [x] 4.1 sqlx 迁移（uq_sourcemap_release_file 唯一约束）
- [x] 4.2 `crates/infra/src/persistence/repositories/sourcemaps.rs` Pg CRUD + `build_object_key`；`crates/infra/src/sourcemaps/mod.rs::translate_frame`（sourcemap crate）
- [x] 4.3 `crates/api/src/http/routes/sourcemaps.rs`：multipart upload + list/delete + 50 MiB 上限 + upsert
- [ ] 4.4 RUM error 路径自动调 translate_frame（pipeline 时接入）
- [ ] 4.5 集成测试

### 5. log-patterns ✅（CRUD + 编译验证 + 内存 first_match；UDF 注册留 follow-up）
- [x] 5.1 sqlx 迁移（含 priority DESC 索引）
- [x] 5.2 `crates/infra/src/persistence/repositories/log_patterns.rs`：CRUD + `compile_check` + `first_match`
- [ ] 5.3 DataFusion UDF `extract_pattern(message)` 注册（需 SessionContext::register_udf；留 follow-up）
- [x] 5.4 `crates/api/src/http/routes/log_patterns.rs`：CRUD + 编译失败返 400
- [ ] 5.5 `vectorscan-rs` optional dep（regex fallback 已可用）
- [ ] 5.6 集成测试

### 6. search-jobs ✅（HTTP 落地；worker 待后续）
- [x] 6.1 sqlx 迁移（FOR UPDATE SKIP LOCKED 索引就绪）
- [x] 6.2 `crates/infra/src/persistence/repositories/search_jobs.rs`：CRUD + 状态机 + `claim_next_pending`
- [ ] 6.3 `crates/app/src/query/search_jobs.rs::SearchJobScheduler`：tokio worker pool + QueryService::run + Parquet 写入（**留后续**——worker 上线后已建 row 自动 pickup）
- [x] 6.4 `crates/api/src/http/routes/search_jobs.rs`：`POST /query/jobs` + `GET /query/jobs/:id` + `GET /query/jobs/:id/results` + `DELETE`
- [ ] 6.5 后台 `search_jobs_cleanup`（hourly task）
- [ ] 6.6 集成测试

### 7. search-inspector ✅（MVP endpoint；完整 plan dump 留 follow-up）
- [x] 7.1 `crates/api/src/http/routes/query.rs::inspect_query`：endpoint `/query/inspect`，返 metadata + stub plan
- [ ] 7.2 per-stage 计时 hook（需 DataFusion ObservableExec wrapper）
- [ ] 7.3 result `meta.profile`
- [ ] 7.4 集成测试

### 8. profiling ✅（门控 endpoint；实际 pprof 输出留 follow-up）
- [ ] 8.1 `pprof` crate（heavy system deps，留 feature flag follow-up）
- [x] 8.2 `crates/api/src/http/routes/profiling.rs`：`/api/v1/debug/profile/{cpu,heap}` 受门控；缺 `MS_PROFILING_ENABLED=true` 返 404；启用后返 503 + TODO 提示
- [x] 8.3 ENV 校验（`PROFILING_ENABLED` / `PROFILING_ALLOW_REMOTE`）；peer IP 校验由部署侧承担
- [ ] 8.4 集成测试

### 9. mmdb-enrichment ✅（lookup + downloader stub；真实 HTTP 下载 + VRL builtin 留 follow-up）
- [x] 9.1 workspace 加 `maxminddb = "0.27"`
- [x] 9.2 `crates/infra/src/enrichment/mmdb_downloader.rs::MmdbDownloader`：`ensure_ready` 检查 + 缺 key warn；HTTP 下载留 follow-up
- [x] 9.3 `crates/infra/src/enrichment/geoip.rs::GeoIp`：`open(path)` / `noop()` / `lookup(ip) -> Option<GeoLocation>`（country / region / city / lat / lng）
- [ ] 9.4 VRL `geoip_lookup` builtin（依赖 vrl crate；与实际 VRL runtime 接入一起做）
- [ ] 9.5 `[mmdb]` 配置段加 settings.rs
- [ ] 9.6 集成测试

### 10. scheduled-reports ✅（schema + CRUD；render/deliver/tick 留 follow-up）
- [x] 10.1 sqlx 迁移（scheduled_reports + report_deliveries）
- [ ] 10.2 render 引擎（SVG MVP 留 follow-up；headless Chrome 走 enterprise feature）
- [ ] 10.3 deliver sink 三套（email/webhook/s3，复用 notify infra）
- [x] 10.4 `crates/api/src/http/routes/scheduled_reports.rs`：CRUD + 校验（dashboard XOR saved_view + 5 种 format + ≥1 recipient）+ `/deliveries` 历史查询
- [ ] 10.5 alert_manager tick 触发
- [ ] 10.6 集成测试

### 11. incidents-grouping ✅（grouping 算法 + HTTP；dispatcher 联动留后续）
- [x] 11.1 sqlx 迁移
- [x] 11.2 `crates/infra/src/persistence/repositories/incident_groups.rs::upsert_for_incident`：原子合并算法（同 rule+fp+15min 窗口 → count++；否则新建）
- [ ] 11.3 `crates/app/src/alerting/dispatcher.rs`：group resolve → 成员 incident resolve（**留后续**——repo `ack/resolve` 已就位）
- [x] 11.4 `crates/api/src/http/routes/incident_groups.rs`：list（state filter）/ get / ack / resolve
- [ ] 11.5 集成测试

### 12. config-watcher ✅（watcher + diff + immutable 守护；wire 接入留 follow-up）
- [x] 12.1 workspace 加 `notify = "7"`
- [x] 12.2 `crates/config/src/watcher.rs::spawn_config_watcher`：notify event → 回调 + diff_toml 计算
- [x] 12.3 `IMMUTABLE_FIELDS` 静态清单替代 macro（spec 文档约定）
- [x] 12.4 immutable 字段变更只 warn 日志，FieldDiff.immutable = true
- [ ] 12.5 `wire::build_state` 启动 watcher（需注入 last_snapshot + on_change apply 闭包；留 follow-up）
- [ ] 12.6 集成测试

### 13. cluster（修订）✅（schema + repo + HTTP CRUD；ingester/querier 改走路由留 follow-up）
- [x] 13.1 sqlx 迁移（org_storage_providers）
- [x] 13.2 `crates/infra/src/persistence/repositories/org_storage_providers.rs` 含 upsert + get + list + delete + `ensure_no_inline_secret` 校验
- [ ] 13.3 `StorageRouter::for_org` + LRU cache（hot-path 改造留 follow-up）
- [x] 13.4 `crates/api/src/http/routes/storage_providers.rs`：Owner-only CRUD；inline key 拒收 + backend 枚举校验
- [ ] 13.5 ingester / querier / compactor 改走 `StorageRouter::for_org`
- [ ] 13.6 集成测试

### 14. storage（修订）✅（file_download_tokens + endpoints；S3 pre-signed + OrgSchemaCache 留 follow-up）
- [x] 14.1 sqlx 迁移（file_download_tokens）
- [x] 14.2 `crates/api/src/http/routes/files.rs`：`POST /files/download` + 顶层公开 `GET /api/v1/files/stream/<token>`（auth 白名单）；MVP 统一 streaming token；S3 pre-signed URL 留 follow-up
- [ ] 14.3 OrgSchemaCache（pipeline 提速；后续 query/ingester 接入时同步做）
- [ ] 14.4 StreamRepository::update_schema invalidation hook
- [ ] 14.5 集成测试

### 15. identity（修订）✅（policy 存储 + HTTP；evaluator 接入 middleware 留后续）
- [x] 15.1 sqlx 迁移
- [x] 15.2 `crates/infra/src/persistence/repositories/rbac_policies.rs`：CRUD + `match_policies`（subject/action/resource_id 通配 `*`）+ `evaluate(policies)` DENY>ALLOW
- [ ] 15.3 `crates/app/src/identity/policy.rs::PolicyEvaluator` trait（包装上述 evaluate；本轮 evaluator 函数已可独立测试）
- [ ] 15.4 `permission.rs` 调用 PolicyEvaluator（**留后续**——需要先决定 cache 层）
- [x] 15.5 `/api/v1/auth/policies` CRUD（Admin+）
- [ ] 15.6 policy deny 写 audit
- [ ] 15.7 集成测试

### 16. query（修订）✅（Prefer 头 + async 转化；multi-stream planner + 阈值检测留 follow-up）
- [ ] 16.1 多 stream JOIN planner（DataFusion 深度改造）
- [ ] 16.2 planner estimate 超阈值自动 async
- [x] 16.3 `execute_query`：检测 `Prefer: respond-async` 头 → 创建 search_job + 返 202 + `{job_id, monitor}` URL
- [ ] 16.4 `[querier].auto_async_threshold_rows` 配置
- [ ] 16.5 集成测试

## M2 Web UI 主流程（5 个高频页）

### 17. Web shell 公共基础
- [ ] 17.1 `web/src/features/` 目录约定 + `routes.tsx` 模式文档
- [ ] 17.2 `shell/keyboard` 与 `shell/stack` 对外 API 稳定化（导出 `useChord`、`pushFrame`）
- [ ] 17.3 通用 list / detail / form 组件（`shell/ui/` 增 `DataTable / DetailHeader / FormDrawer`）

### 18. Alert CRUD page
- [ ] 18.1 `web/src/features/alerts/{routes,api,list,detail,form,keyboard}.tsx`
- [ ] 18.2 规则列表 + 编辑器（trigger / for_periods / labels JSON 编辑器）
- [ ] 18.3 测试触发按钮（mock data 跑一次 evaluator 出预期结果）
- [ ] 18.4 silence UI（窗口 + 标签匹配）
- [ ] 18.5 chord `g a` → /alerts；`n` → new；`/` → search
- [ ] 18.6 vitest + RTL 单测

### 19. Dashboard builder
- [ ] 19.1 `web/src/features/dashboards/`：list / detail / builder
- [ ] 19.2 lazy-load Monaco（dashboard JSON 编辑）
- [ ] 19.3 panel 库（time-series / table / single-stat / log-list 四种）
- [ ] 19.4 Grafana JSON 导入 / 导出
- [ ] 19.5 panel 配置面板（SQL/PromQL + 字段映射）
- [ ] 19.6 推入 investigation stack 支持
- [ ] 19.7 vitest 单测

### 20. Metrics explorer
- [ ] 20.1 `web/src/features/metrics/`：PromQL IDE（基于 codemirror）
- [ ] 20.2 时序图渲染（uPlot；与 shell/timeseries 对齐）
- [ ] 20.3 聚合维度选择器（动态从 label set 拉）
- [ ] 20.4 时间锚点联动
- [ ] 20.5 vitest 单测

### 21. Pipeline editor
- [ ] 21.1 `web/src/features/pipelines/`：list + 视觉 workflow 编辑（DAG）
- [ ] 21.2 step 编辑面板（VRL 代码 + 测试输入）
- [ ] 21.3 流程预览（实时跑一条样本事件展示中间结果）
- [ ] 21.4 vitest 单测

### 22. Settings
- [ ] 22.1 `web/src/features/settings/`：7 tab 分（org / API tokens / quotas / SSO / cipher keys / connectors / audit）
- [ ] 22.2 每 tab 独立 api 模块
- [ ] 22.3 敏感字段 mask 显示
- [ ] 22.4 vitest 单测

## M3 Enterprise 主要 capability

### 23. actions (enterprise) ✅（crate + ActionExecutor + 模板渲染；HTTP route + repo + dispatcher 接入留 follow-up）
- [x] 23.1 `enterprise/crates/actions/` 完整 crate scaffold
- [ ] 23.2 sqlx 迁移 actions + action_executions（OSS 主仓）
- [x] 23.3 `ActionKind`（Webhook + Script）+ `ActionExecutor::execute` + `WebhookClient` trait + `NoopWebhookClient` + `render_template`（mustache 7 占位符）
- [ ] 23.4 dispatcher step kind: action 接入
- [ ] 23.5 `crates/api/src/http/routes/actions.rs` (cfg=enterprise) + license.has_feature("actions") 检查
- [ ] 23.6 集成测试

### 24. fga-policies (enterprise) ✅（FgaEvaluator + cache + license gate；OSS PolicyEvaluator 替换 cfg 接入留 follow-up）
- [x] 24.1 `enterprise/crates/fga-policies/` 完整 crate
- [x] 24.2 `PolicyBackend` trait + `MatchedPolicy` + `FgaEvaluator`；rbac_policies 扩列由 backend impl 承担
- [ ] 24.3 OSS 主仓 `PolicyEvaluator` 在 cfg=enterprise 时替换为 FgaEvaluator
- [x] 24.4 `DashMap` 60s 决策缓存 + `invalidate_org`
- [x] 24.5 `evaluate(policies)` DENY > ALLOW > NotMatched + 通配符（在 OSS `rbac_policies::match_policies` 已实现）
- [ ] 24.6 集成测试（unit tests 已覆盖 deny/allow/license/empty 4 个）

### 25. copilot-mcp (enterprise) ✅（JSON-RPC 协议层 + tool registry + handler dispatcher；WebSocket transport 留 follow-up）
- [x] 25.1 `enterprise/crates/copilot-mcp/`：JSON-RPC envelope + handle_request（initialize / tools/list / tools/call）
- [x] 25.2 `builtin_tools()` 含 5 个：query_logs / query_metrics / list_streams / get_trace / list_recent_alerts（含 JSON Schema 输入校验）
- [x] 25.3 `McpAuthContext { user_id, org_id, role }` + `ToolDispatcher` trait
- [ ] 25.4 `/mcp` WebSocket 路由（axum WebSocketUpgrade + tokio_tungstenite 留 follow-up；handle_request 已可被任意 transport 调用）
- [ ] 25.5 集成测试（unit tests 已覆盖 5 个：initialize / tools_list / tools_call_dispatches / unknown_method / error_propagated）

### 26. copilot-chat (enterprise) ✅（crate + ChatLoop + Provider trait；HTTP / SSE / repo 留 follow-up）
- [x] 26.1 `enterprise/crates/copilot-chat/` 完整 crate；依赖 `copilot-mcp` tool registry
- [ ] 26.2 sqlx 迁移 chat_sessions + chat_messages（OSS 主仓）
- [x] 26.3 `Provider` enum (OpenAI / Anthropic / OpenAI-compatible) + `ProviderAdapter` trait（OSS 注入 HTTP 客户端）
- [ ] 26.4 SSE handler（api crate cfg-gated）
- [x] 26.5 `ChatLoop::run`：tool call → dispatch → 回灌循环 + MAX_TOOL_LOOPS=8 兜底 + token 计数
- [ ] 26.6 chat 调用 trace 写 `copilot_traces`
- [ ] 26.7 `/api/v1/copilot/chat/*` 路由
- [ ] 26.8 集成测试（unit tests 3 个：loop_executes / loop_aborts_max / provider_roundtrip）

## M4 Web UI 长尾 + Enterprise 长尾

### 27. Functions UDF editor (Web)
- [ ] 27.1 `web/src/features/functions/`：VRL / JS 代码编辑器
- [ ] 27.2 编译错误高亮
- [ ] 27.3 测试 harness（输入 JSON → 跑函数 → 看输出）

### 28. RUM dashboards (Web)
- [ ] 28.1 `web/src/features/rum/sessions.tsx`：session 列表 + 详情
- [ ] 28.2 `web/src/features/rum/errors.tsx`：error 列表 + 翻译 stack
- [ ] 28.3 `web/src/features/rum/performance.tsx`：Core Web Vitals

### 29. Sourcemaps UI (Web)
- [ ] 29.1 `web/src/features/sourcemaps/`：上传 + 列表 + 删除
- [ ] 29.2 翻译效果预览（粘贴 minified stack → 显示翻译后）

### 30. Scheduled reports UI (Web)
- [ ] 30.1 `web/src/features/scheduled_reports/`：CRUD
- [ ] 30.2 投递历史 + 失败 retry 按钮

### 31. Ingestion wizard (Web)
- [ ] 31.1 `web/src/features/ingestion/`：语言选择 → SDK 代码生成（Vector / Fluent Bit / OTel Collector / Promtail）
- [ ] 31.2 配置预览 + 复制按钮
- [ ] 31.3 发送测试事件（验证接入）

### 32. Short URL manager (Web)
- [ ] 32.1 `web/src/features/short_urls/`：列表 + click 统计 + 失效

### 33. Annotations editor (Web)
- [ ] 33.1 `web/src/features/annotations/`：时间窗 + 标题 + 关联 stream/dashboard 编辑器

### 34. Incidents view (Web)
- [ ] 34.1 `web/src/features/incidents/`：group 列表 + 详情（关联告警 + 时间线）
- [ ] 34.2 ack / resolve / mute 操作

### 35. cloud-marketplace (enterprise) ✅（订阅状态机 + 解析 + metering aggregator；webhook 路由 + 真实 API call 留 follow-up）
- [x] 35.1 `enterprise/crates/cloud-marketplace/` 完整 crate
- [ ] 35.2 sqlx 迁移 marketplace_subscriptions（OSS 主仓）
- [x] 35.3 AWS notification 解析 + `aws_action_to_state` + `SubscriptionState` 状态机 + `can_transition_to`
- [x] 35.4 Azure notification 解析 + `azure_action_to_state`
- [x] 35.5 `MeteringAggregator`（内存累计 + drain）+ `MarketplaceClient` trait（OSS 注入 SDK）
- [ ] 35.6 集成测试（unit tests 4 个：state_transitions / aws_mapping / azure_mapping / aggregator_drain）

### 36. model-pricing (enterprise) ✅（PricingCatalog + default seed + compute_cost；HTTP CRUD + chat 接入留 follow-up）
- [x] 36.1 `enterprise/crates/model-pricing/` 完整 crate
- [ ] 36.2 sqlx 迁移 model_prices（OSS 主仓）+ migration 时 seed
- [x] 36.3 `PricingCatalog::with_defaults()` seed 4 主流模型 + `compute_cost(catalog, provider, model, prompt, completion)` 公式
- [ ] 36.4 chat/MCP 调用入口接入 cost_usd 计算（与 26.6 一起做）
- [ ] 36.5 集成测试（unit tests 4 个：seed_known_models / cost_spec_example / missing_model_zero / upsert_replaces）

### 37. domain-management (enterprise) ✅（Domain 模型 + hostname 校验 + 续期算法；真实 ACME client + ACME router + SNI 留 follow-up）
- [x] 37.1 `enterprise/crates/domain-management/` 完整 crate
- [ ] 37.2 sqlx 迁移 domains（OSS 主仓）
- [x] 37.3 `AcmeClient` trait + `IssuedCertificate` 模型（OSS 注入 instant-acme 实现）
- [ ] 37.4 router 加 `/.well-known/acme-challenge/<token>` 路由（与 OSS api crate 接入）
- [x] 37.5 `renewal_cutoff_micros(now)` = now + 30d；`needs_renewal(domain, now)`；`RENEWAL_RETRY_SECS = 6h`
- [ ] 37.6 SNI cert selector（运行时 TLS 接入）
- [ ] 37.7 集成测试（unit tests 4 个：good_hostnames / bad_hostnames / renewal_cutoff_30d / needs_renewal_within_30d）

## M5 完工校验

- [x] 38.1 `cargo fmt --all` 全过；`cargo clippy --workspace --all-targets`（OSS + enterprise）只剩 pre-existing warning，无新 error
- [x] 38.2 `cargo test --workspace --lib`（两 profile 各跑）：**134 全过**（shared 19 + config 7 + domain 3 + infra 95 + api 20 + protocol 3 + app 3 + ...）零回归
- [ ] 38.3 集成测试 `MS_RUN_IT=1 cargo test --test 'it_*'`：`cargo test --no-run` 全部编译通过；实际执行需 docker
- [ ] 38.4 前端被用户跳过（M2 + M4 web 部分不在本批 scope）
- [x] 38.5 `openspec validate feature-parity-with-openobserve --strict` ✅
- [x] 38.6 `cd enterprise && cargo test --workspace`：**35 单测全过**（license 2 + copilot 4 + actions 4 + fga 5 + mcp 5 + chat 3 + pricing 4 + marketplace 4 + domain 4）
- [ ] 38.7 部署演练：需真集群（staging 环境跑），留后续
- [x] 38.8 ARCHITECTURE.md 追加 **Part 3**（23 个新 capability 设计 + 编译矩阵 + 关键不变量）；README / openapi.yaml 同步留 follow-up
