## Why

我们刚归档了 `production-core-engine`（22 capability，把 OO 核心算力一次性补齐），但相对 OpenObserve（OO）真实生产形态仍欠两块：(1) **十多个用户面/运维面的小能力**（短链、注解、源映射、模式提取、async search jobs、profiling、scheduled reports、incident grouping、config 热重载 等），缺一项就会在 HN/Reddit 评论被立刻指出"看起来像个 demo 而不是产品"；(2) **完整 Web UI**——目前只有 `web-investigation-shell` 在 spec 化 ⌘K + 调查栈 + 跨信号关联 + 拓扑 + log/trace view + 时间锚点，但 Alert / Dashboard builder / Metrics explorer / Pipeline editor / RUM dashboards / Settings / Functions UDF editor / Ingestion wizard 这一票 CRUD 页面完全缺失。1.0 发布之前必须把这两块补齐，不然不能算 OO 同等量级，README 的对比表也站不住。

按用户偏好（`molesignal-scope-stance` memory）：单一 change 一次性纳入，不切小；纳不进的（如 OSS vs Enterprise 的责任边界）通过 design.md 内分阶段说明。

## What Changes

### A. OSS 主仓 12 个新 capability

- **functions-runtime**：HTTP CRUD + VRL 编译 + JS feature gate（`functions` domain stub 已有，本 change 补完）
- **short-url**：`/api/v1/short` 短链生成 + 反解，dashboard / saved view / 报告分享必需
- **annotations**：时间序列图表叠加（deployment marker / maintenance window）
- **sourcemaps**：JS 源码 map 上传 + RUM stacktrace minified → original 翻译
- **log-patterns**：regex 模式提取与归类（feature `vectorscan` 可选加速）
- **search-jobs**：长查询异步队列 + 轮询结果，与现有 `query` 协作
- **search-inspector**：查询计划 + 性能 profiling endpoint（`EXPLAIN` 加强版）
- **profiling**：`/debug/profile/{memory,cpu}` pprof 二进制输出（jemalloc / tikv 集成）
- **mmdb-enrichment**：MaxMind GeoIP 下载 + IP → location 查询（含 VRL builtin）
- **scheduled-reports**：dashboard 周期 email / webhook 投递（基于 `scheduled-pipelines` 思路但 target 是渲染输出）
- **incidents-grouping**：alert 聚合算法（reduce 告警噪音；与 `alerting` incidents 模型对接）
- **config-watcher**：TOML 配置 inotify 热重载（含安全字段守护）

### B. OSS 修订 4 个现有 capability

- **cluster**：扩 `org_storage_providers`——per-org bucket 路由（不同 org 落不同 S3 / GCS endpoint）
- **storage**：扩 `file_downloader` 异步下载 + `org_schema_cache` 内存 schema 缓存
- **identity**：扩 RBAC policy 存储基础（细粒度 policy 表 + decision cache；完整 FGA engine 由企业版扩展）
- **query**：multi-stream SQL search（单 SQL 跨 stream JOIN）+ search_jobs 调度集成

### C. Enterprise 新增 7 个 capability（落 `enterprise/crates/`）

- **actions**：alert 触发的脚本 / webhook 执行；workflow-engine 风格的 step 编排
- **copilot-mcp**：Model Context Protocol server（Claude / Cursor / Continue 等客户端可经 MCP 调用 molesignal 查询）
- **copilot-chat**：对话式查询接口（自然语言 → SQL/PromQL + RAG 经 copilot_traces）
- **cloud-marketplace**：AWS / Azure Marketplace 订阅 + metering API
- **model-pricing**：Copilot 按 token 计费 + 模型成本表
- **domain-management**：自定义域名 + SSL 证书（multi-tenant SaaS 部署用）
- **fga-policies**：完整 FGA engine（resource-level RBAC，e.g. `stream:read:org1/stream_x`）

### D. Web UI 全套 CRUD 页面（沿用 `web-investigation-shell` 设计原则）

新增 React 视图，纳入主 web 工作区。每个页面都同时满足键盘优先 + investigation-stack 兼容（视图可被推入栈、可被 ⌘K 召回）：

- **Alert CRUD page**：规则列表 + 编辑器 + 测试触发 + silence
- **Dashboard builder**：drag-drop 面板布局 + 可视化类型选择器 + Grafana JSON 导入导出
- **Metrics explorer**：PromQL IDE + 时序图构建 + 聚合维度选择
- **Pipeline editor**：visual workflow（function step 编辑 + VRL 语法高亮 + 测试输入）
- **Functions UDF editor**：VRL / JS 代码编辑 + 编译错误高亮 + 测试 harness
- **RUM dashboards**：sessions / errors / performance 三视图（含 session replay 占位）
- **Sourcemaps upload UI**：拖入上传 + 版本对齐 + 翻译效果预览
- **Scheduled reports UI**：报告订阅 CRUD + 渲染预览 + 投递历史
- **Settings**：org info / API tokens / quotas / SSO config / cipher keys / connectors 七 tab
- **Ingestion wizard**：选语言 + SDK 代码段生成 + agent 配置示例（Vector / Fluent Bit / OTel Collector）
- **Short URL manager**：列表 + click 统计 + 失效控制
- **Annotations editor**：时间窗 + 标题 + 关联 stream / dashboard
- **Incidents view**：分组列表 + 关联告警 + ack / resolve / mute 操作

## Capabilities

### New Capabilities

- `functions-runtime`: HTTP CRUD + VRL / 可选 javascript runtime + 编译错误返 400 + 函数 chain 在 ingest path 调用
- `short-url`: kebab-case 短链生成 + 反解 + click 计数 + 失效策略
- `annotations`: 时间窗注解 CRUD + dashboard / chart 上的叠加渲染元数据
- `sourcemaps`: JS source map 上传 + stacktrace 翻译 + 与 RUM error 关联
- `log-patterns`: 正则模式 CRUD + 命中分类 + 性能优化（vectorscan optional）
- `search-jobs`: 长查询 job 表 + 异步执行 + 轮询 endpoint + 结果持久化
- `search-inspector`: 查询计划 dump + per-stage 耗时 + 扫描数据量统计
- `profiling`: pprof 二进制端点（jemalloc heap + cpu samples）
- `mmdb-enrichment`: GeoIP DB 下载调度 + IP 查询 + VRL `geoip_lookup` builtin
- `scheduled-reports`: dashboard / saved view 渲染 + 周期投递（email / webhook / S3）
- `incidents-grouping`: alert → incident 聚合算法 + 去重 + root-cause 提示
- `config-watcher`: TOML 配置 inotify watch + 安全字段过滤 + 热重载
- `actions`: 脚本 / webhook step + 执行 context + 结果落审计
- `copilot-mcp`: Model Context Protocol server 实现 + tool registry
- `copilot-chat`: 自然语言查询 + RAG over copilot_traces + 流式响应
- `cloud-marketplace`: AWS / Azure 订阅生命周期 + metering 上报
- `model-pricing`: 模型成本表 + token 计费 + 配额对账
- `domain-management`: 自定义 hostname + Let's Encrypt 证书 + 路由
- `fga-policies`: per-resource policy 表 + 评估 engine + 缓存层
- `web-shell-crud`: 13 个 CRUD 页面统称（每页含 list / detail / create / edit / delete / keyboard hotkeys / investigation-stack 集成）

### Modified Capabilities

- `cluster`: 加 per-org `storage_providers` 路由表 + bucket 选择 API
- `storage`: 加 `file_downloader` 异步下载 service + `org_schema_cache` 内存缓存
- `identity`: 加 RBAC policy 存储 + decision cache + 中间件接入（完整 FGA engine 由 `fga-policies` 企业 capability 提供）
- `query`: 加 multi-stream SQL（单 SQL JOIN 多 stream）+ search_jobs 调度集成（长查询自动转 async）

## Impact

- **代码体量**：约 ~15000-20000 行 Rust + ~10000 行 React/TS。**这是一份多季度的工作量**，design.md 会把它分阶段（M1 OSS 核心新增 → M2 Web UI 主流程 → M3 Enterprise 主要 capability → M4 Web UI 长尾 + Enterprise 长尾）。
- **依赖新增**：`vectorscan`（optional, log-patterns）、`pprof` / `jemalloc-sys`（profiling）、`maxminddb`（mmdb-enrichment）、`acme-lib`（domain-management 企业）、`oauth2` 加固（copilot-mcp / chat）、`@grafana/scenes` 或自实 dashboard builder（web）、`monaco-editor` 或 `codemirror`（functions / pipeline 编辑器）。
- **数据库**：约新增 12 张表（short_urls / annotations / sourcemaps / log_patterns / search_jobs / mmdb_metadata / scheduled_reports / report_deliveries / incidents_groups / fga_policies / actions / model_prices）+ 现有 `identity` / `cluster` / `storage` 扩列。
- **HTTP API**：约 80+ 新 endpoint（每个 OSS 新 capability 5-8 endpoint，企业版同量级）。
- **Web routes**：约 30+ 新页面（13 个 CRUD 主页 × 平均 2-3 子路由）。
- **License gating**：所有企业 capability 入口加 `license.has_feature(...)` 校验；OSS 默认 false → 403。
- **企业版 crate 增长**：`enterprise/crates/` 从当前 2 个（license / copilot）扩到 ~9 个。
- **Web 工作区扩展**：当前主要是 `web-investigation-shell`；本 change 在同一 React workspace 下新增 13 个 feature 模块，复用 `shell/keyboard` / `shell/stack` 基础设施。
- **测试**：约 30+ 套新集成测试；新 Web 视图加 component-level 测试（vitest + RTL）。
- **文档**：ARCHITECTURE.md 追加 Part 3（13 个新 capability 设计）；docs/ 加 functions / mcp / cloud-marketplace 各自专题；README 对比表新增 "Cost attribution" / "SDK setup" / "Self-hosted custom domain" 几行。
- **非目标**：UI 视觉打磨（仅功能可用，视觉留单独 design polish change）、移动端适配、i18n（先英文 + 中文）、迁移 OO 现有部署的数据导入工具（用户实际不会从 OO 切过来，只参照其能力）。
