## Context

`production-core-engine` 已归档，把 OO 的核心算力（ingest 多协议 / 列存 + Tantivy / 分布式查询 / 多租户 / 告警 / 缓存 / cipher / audit / quotas / RUM / service graph / anomaly / SSO / federated / connectors / scheduled pipelines）一次性补齐。同期 `web-investigation-shell` 在 spec 化键盘流 + 跨信号关联的前端骨架。

本 change 是**走向 1.0 的最后一公里**：补齐 OO 在生产形态下还有但 molesignal 缺的 13 个 backend capability + 7 个企业版 capability + 13 个 Web UI 主页面。

**当前 molesignal 结构**：

```
src/{shared,config,protocol,domain,app,infra,api,bootstrap}
enterprise/crates/{license,copilot}              ← cargo git dep + cfg gate
openspec/specs/                                  ← 22 capability
web/src/                                         ← 主要是 investigation shell
```

**OO 体量参照**（截至本提案）：
- `src/service/` ~30 个子模块
- `src/handler/http/request/` ~40 个 endpoint module
- `src/job/` ~20 个后台 loop
- `web/src/views/` ~50 个 Vue 顶层视图

观察到的 OSS / Enterprise 边界（OO 用 `#[cfg(feature = "enterprise")]` + `o2_enterprise` 外部 crate 划分）：
- OSS：core telemetry + alerting + dashboards + pipelines + RUM
- Enterprise：AI/Copilot/MCP、actions/workflow、cloud marketplace、domain mgmt、license、FGA、search inspector 高级特性、pricing

## Goals / Non-Goals

**Goals:**
- 1.0 发布时**用户能在 README 对比表的每一行都站住**（cost / cross-signal / setup / ownership / OTel-native / realtime / multi-tenant + 新加的 cost attribution / SDK setup / custom domain）
- 主仓 OSS 编译路径**永远不依赖** enterprise/——`cargo build` 必须能在公网无 SSH key 的情况下完整跑通
- Web UI 13 个 CRUD 页面**全键盘可达 + 可被 ⌘K 召回 + 可推入 investigation stack**（与 `web-investigation-shell` 的 KeyboardController + InvestigationStack 复用）
- 每个新 capability **不破坏既有 crate 边界**：domain port + app service + infra repo + api handler 四件套
- 企业版每个新 capability **通过 `license.has_feature(...)` 校验**；OSS 编译时 cfg 屏蔽，不留 dead code

**Non-Goals:**
- UI 视觉打磨（functional 即可，视觉单独 change）
- 移动端适配 / i18n（中英双 README 已覆盖，UI i18n 留后续）
- OO 数据迁移工具（用户大概率不会从 OO 切来；只参照能力）
- profiling 的 continuous 模式（一次性 dump 足够，常驻探针留后续）
- copilot-chat 的本地 LLM 部署（默认走 OpenAI / Anthropic API；自托管 ollama 留后续）
- dashboard 编辑器的"无代码全功能"（先实现 Grafana 风格的 panel + JSON 编辑双轨；可视化编辑器留后续）

## Decisions

### D1：按 milestone 分阶段实施，但只走一个 change

按用户偏好（`molesignal-scope-stance` memory），单 change 一次性纳入；但落地内部分 4 个 milestone，tasks.md 按 milestone 分章节，每 milestone 可独立 PR / 验证：

- **M1 OSS 核心新增**（12 capability + 4 修订）：functions-runtime / short-url / annotations / sourcemaps / log-patterns / search-jobs / search-inspector / profiling / mmdb-enrichment / scheduled-reports / incidents-grouping / config-watcher + cluster/storage/identity/query 扩展
- **M2 Web UI 主流程**（5 个高频页）：Alert CRUD / Dashboard builder / Metrics explorer / Pipeline editor / Settings
- **M3 Enterprise 主要 capability**（4 个）：actions / fga-policies / copilot-mcp / copilot-chat
- **M4 Web UI 长尾 + Enterprise 长尾**（8 + 3）：Functions UDF editor / RUM dashboards / Sourcemaps UI / Scheduled reports UI / Ingestion wizard / Short URL manager / Annotations editor / Incidents view + cloud-marketplace / model-pricing / domain-management

**理由**：单 change 利于"一次评审、一次架构对齐"，但 15000-20000 行的实施不能等所有都完才合；按 milestone 推进 + tasks.md 标号反映顺序，能让人随时知道进度。

**备选**：拆 4 个 change（each milestone 一个）。**否决**——用户明确反对切小，且 capability 间存在依赖（incidents-grouping 用 alerting；scheduled-reports 用 short-url 等）。

### D2：Web 视图统一架构 — feature 模块 + shell 共享层

每个 CRUD 页面是一个 React feature 模块（`web/src/features/<name>/`），强制对接两个 shell 基础设施：
1. `shell/keyboard/KeyboardController` 注册自己的 chord 与 hotkey
2. `shell/stack/InvestigationStack` 允许被 push 入栈（pinned 时不被挤掉）

页面结构统一：
```
features/<name>/
├── routes.tsx              ← lazy-loaded React Router 块
├── api.ts                  ← TanStack Query hooks
├── list/index.tsx          ← list view
├── detail/index.tsx        ← detail view + edit drawer
├── form.tsx                ← shared create/edit form
└── keyboard.ts             ← chord 注册（如 `g a` go to alerts）
```

**理由**：与 `web-investigation-shell` 设计原则一致；新页面对接键盘流是硬约束，避免做出"鼠标驱动的孤岛 page"。

**备选**：每个页面独立路由 + 自定义内部 state。**否决**——会破 shell 的单一交互模型。

### D3：Async search jobs 复用现有 query engine

`search-jobs` 不重写查询引擎；只在 `app/query` 与 `api/http/routes/query` 之间加一层调度：

```
POST /api/v1/query           (synchronous; existing)
POST /api/v1/query/jobs      (async; this change)
  → Insert into search_jobs table (state=Pending)
  → Spawn tokio task → reuse QueryService::run
  → Update state=Running / Done / Failed + result_object_key

GET  /api/v1/query/jobs/:id  → poll status + (if Done) link to result
GET  /api/v1/query/jobs/:id/results?page=  → paginate result (Parquet on object_store)
```

结果以 Parquet 写到 object store（`query_jobs/<job_id>.parquet`），由后台 ttl job 清理。

**理由**：避免重复实现执行路径；结果走 Parquet 让分页和大结果集天然支持。

**备选**：流式（NDJSON streaming 已有）替代 async jobs。**否决**——streaming 客户端需要长连接保持，async jobs 更适合"分析师晚上跑、第二天看"的 OLAP 场景。

### D4：Enterprise capability 走 cfg gate + 独立 cargo workspace

继续沿用 `production-core-engine` 确立的模式：
- `enterprise/Cargo.toml` 是独立 cargo workspace
- 主仓 `Cargo.toml` 的 enterprise crates 用 `git = "ssh://git@github.com/molesignal/molesignal-enterprise.git"` + 顶层 `[patch]` 指向本地 `enterprise/`
- 主仓代码用 `#[cfg(feature = "enterprise")]` 控制 import 与路由注册
- handler 一律 `license.has_feature(<key>)`；OSS 永远 false → 403

本 change 新加的 7 个企业 capability 全部按此模式落到 `enterprise/crates/`：
```
enterprise/crates/
├── license/             (已有)
├── copilot/             (已有)
├── actions/             (本 change)
├── fga-policies/        (本 change)
├── copilot-mcp/         (本 change)
├── copilot-chat/        (本 change，依赖 copilot-mcp 的 tool registry)
├── cloud-marketplace/   (本 change)
├── model-pricing/       (本 change)
└── domain-management/   (本 change)
```

**理由**：边界清晰、不污染 OSS、license gate 是单点。

**备选**：把企业代码留在主仓 `crates/`，只用 `cfg(feature)` 控制。**否决**——OSS clone 的人能看到企业代码，违反商业边界。

### D5：Copilot-chat 的 RAG 数据源 = `copilot_traces` 派生流

`copilot-chat` 不引入新数据通路；用户的自然语言提问经 LLM 转 SQL/PromQL → 经 `QueryService::run` → 结果回流给 LLM 拼答案。整个对话被 trace 化写入 `copilot_traces`（`gen_ai.*` 属性），形成自闭环：

- 平台自己的 copilot 调用 → 经 `CopilotFanoutHook::extract` 写 `copilot_traces`
- 后续用户问 "上周 copilot 跑了多少 token" → SQL on `copilot_traces` → 答案

**理由**：复用已建的 fanout / redact / stats 设施；不需要为 chat 引入新存储。

### D6：FGA policy 存储分两层

- OSS：role-based policy 表（`identity` capability 扩）——`(org_id, role, resource_kind, action)` 索引
- Enterprise：fga-policies capability 加 attribute-based + relationship-based 评估（参考 OpenFGA model）

OSS 提供 `PolicyEvaluator` trait，企业版替换实现。

**理由**：OSS 用户能用基础 RBAC；企业用户买的是表达力（"user A can read stream X but not stream Y in same org"）。

### D7：Web `Dashboard builder` MVP = Grafana JSON 双轨

不实现"无代码全可视化"编辑器（工作量过大）；MVP 走两条轨：
1. **JSON 编辑器**（Monaco / CodeMirror）—— 直接编辑 dashboard JSON，schema 校验
2. **预设 panel 库** —— 时序图 / 表格 / 单值 / 日志列表 4 种 panel；用户在 panel 配置面板填 SQL / PromQL + 维度

完整 drag-drop 可视化编辑器留单独 change。

**理由**：Grafana JSON 兼容是已交付能力；JSON 编辑 + 预设 panel 已能 cover 90% 用例。

### D8：所有新 capability 必须含 metric + audit 接入

每个新 capability 上线时：
- 至少 1 个 prometheus counter / histogram family（如 `short_url_lookups_total`、`search_job_state_total{state}`）
- 所有 mutating endpoint 自动经现有 `audit_layer` 落 `audit_events`
- handler `Permission::require(...)` 显式（避免漏权限校验）

**理由**：避免"加了功能但运维盲区"的反模式。

## Risks / Trade-offs

- **[Risk] 体量过大单 PR 难合**：15-20k 行 Rust + 10k 行 React。**Mitigation**：M1-M4 各自独立可合并；tasks.md 标 milestone 边界；每 milestone 结束跑一次 `cargo test --workspace --lib` 全过。
- **[Risk] Web UI 与 web-investigation-shell 抢 React workspace 资源**：两者同期开发可能冲突 import / route 命名。**Mitigation**：在 `web/src/features/` 下按 feature 隔离；route prefix 严格按 spec 划分（`/alerts/*` / `/dashboards/*` 等）；提前在本 design 确认 feature 模块结构。
- **[Risk] Profiling 端点暴露内存敏感数据**：pprof heap dump 含字符串内容。**Mitigation**：profiling endpoint 默认仅 localhost 可访问；prod 部署需显式开启 `MS_PROFILING_ENABLED=true`。
- **[Risk] MMDB 下载侵犯 MaxMind 服务条款**：GeoLite2 需 license key + 服务条款 acceptance。**Mitigation**：不打包 GeoLite2；用户运维侧自配 `MS_MMDB_LICENSE_KEY`；缺失时 `geoip_lookup` 返 null（不阻塞 ingest）。
- **[Risk] Search jobs 结果 parquet 占满 object store**：用户跑大查询不取结果。**Mitigation**：默认 7 天 TTL；后台 `search_jobs_cleanup` 任务每小时扫 + `mark_deleted` + object_store delete；用户可手动延长。
- **[Risk] Config-watcher 误重载敏感字段**：JWT secret / master_key 等不能动态切。**Mitigation**：`Settings` 字段标注 `#[hot_reloadable(false)]`；watcher 比对 diff，发现 immutable 字段变了仅 warn 不重载。
- **[Risk] FGA OSS 与 enterprise 版本互不兼容**：OSS 写的 role policy 在 enterprise 升级后语义改变。**Mitigation**：OSS 的 policy 表 schema 是 enterprise 的真子集；enterprise 加列不改列；迁移脚本只在企业版加列。
- **[Risk] AWS / Azure Marketplace API 测试困难**：需要真实 marketplace 沙箱账号。**Mitigation**：integration test 用 mockito 模拟 metering endpoint；真实联调放在 staging 私有部署。
- **[Risk] Copilot-MCP 协议会迭代**：MCP spec 自身在演进。**Mitigation**：实现层用 trait 抽 MCP version；首版对齐 MCP 0.1.x，后续 spec 升级单独 change。
- **[Risk] Web Dashboard builder 用 Monaco 增加 bundle 体积**：Monaco ~3MB。**Mitigation**：lazy-load；初始 chunk 不含 Monaco；用户进 dashboard builder 时才下载。

## Migration Plan

无需数据迁移（capability 全新；扩列走 sqlx migration `IF NOT EXISTS` / `ADD COLUMN`）。

部署侧需注意：
- `master_key`、`mmdb license_key`、`marketplace credentials` 等新 secret 加到 k8s Secret manifest
- `[profiling]`、`[mmdb]`、`[reports]`、`[domain]` 新配置段加到 ConfigMap
- enterprise feature 升级用户需要 deploy key 拉私有仓

回滚：每个 milestone 是一组独立 PR；遇问题回滚某个 milestone 不影响其它已合并 milestone。

## Open Questions

1. **Dashboard builder 是否要支持 Mantine 内置图表？** molesignal web 用 Mantine，OO 用 Vue + Plotly。Mantine 自身的 chart 能力有限；选 Plotly / uPlot / Recharts？倾向 uPlot（轻、快），但与 `web-investigation-shell` 已选的 timeseries 库要对齐。
2. **MCP server 是直挂主进程的 grpc 5082 还是单独 role？** 倾向直挂；MCP 流量低，单独 role 增加部署复杂度。
3. **scheduled-reports 的渲染引擎**：headless Chrome 渲染 dashboard PNG 还是 server 端用 SVG 生成？前者更"真"但运维重；后者轻但只能渲染时序图。倾向 SVG MVP + headless Chrome 留企业版。
4. **FGA OSS 部分的"基础"边界**：哪些 policy 算 OSS、哪些算企业版？建议：单一资源类型的简单 ACL（user X can read stream Y）OSS；多资源关系（trace 关联的所有 logs）企业版。
5. **Copilot-chat 默认模型**：OpenAI 还是 Anthropic？还是按用户配置走？倾向 "用户自配 + 默认提示是 'select your provider'"。
