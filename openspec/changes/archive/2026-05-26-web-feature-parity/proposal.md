## Why

跟 `/Users/gagral/code/openobserve` 这套上游前端逐条对照后发现 molesignal web 缺口很大 —— 我们前端只有 17 个路由，openobserve 约 70+ 个。完全缺 6 个大模块（RUM / Action Scripts / Functions+Enrichment / Pipeline Editor / IAM / 多数 Settings 子页）；已有的 11 个主页面也少了多个二级路由（trace details、search-job inspector、PromQL builder、dashboard import、alert history/insights/anomaly、stream explorer 等）。

后端 `crates/api/src/http/routes/` 里这些功能的路由大部分已经在了（`rum.rs` / `identity.rs` / `fga-policies.rs` / `cipher_keys.rs` / `functions.rs` / `scheduled_pipelines.rs` / `storage_providers.rs` / `domains.rs` / `actions.rs` / `short_url.rs` 等），是前端没接入。

本 change 做一次系统性对齐：拉一份完整 sitemap，按 P0/P1/P2 排优先级，分阶段补齐 50+ 个路由，让 molesignal 在功能可见性上至少跟 openobserve 平齐。

## What Changes

### P0（首批 land）—— 完全缺的高价值模块

- **RUM（Real User Monitoring）** 新增 7 个路由：
  - `/rum/sessions` + `/rum/sessions/view/:id`
  - `/rum/errors` + `/rum/errors/view/:id`
  - `/rum/performance/{overview,web-vitals,errors,apis}` —— 4 个子页
  - `/rum/source-maps` + `/rum/upload-source-maps`
  - 接后端 `crates/api/src/http/routes/rum.rs` + `sourcemaps.rs`

- **Functions + Enrichment Tables** 新增 2 个路由：
  - `/functions` —— VRL 函数库列表 + 编辑
  - `/enrichment-tables` —— lookup 表
  - 接 `routes/functions.rs`

- **Pipeline Editor / History / Backfill** 在现 `/pipelines` 基础上加 5 个子路由：
  - `/pipelines/add` + `/pipelines/:id/edit` —— 可视化编辑器
  - `/pipelines/:id/history` —— 执行历史
  - `/pipelines/:id/backfill` —— 历史回填
  - `/pipelines/import` —— 配置导入
  - 接 `routes/scheduled_pipelines.rs`（缺的子端点需后端补，作为 follow-up tracked）

- **IAM** 新增独立 `/iam/*` 7 个子路由：
  - users、service-accounts、organizations、groups、roles、quota、invitations
  - 接 `routes/identity.rs` + `routes/rbac_policies.rs` + `routes/license.rs`
  - 复用 `web-admin-pages` 已有的 admin 骨架

### P1 —— Settings 16 个子页

`/settings/*` 下扩到 16 个子路由：
- general、organization、license（已有）、alert_destinations、alert_templates、pipeline_destinations、cipher_keys、regex_patterns、ai_toolsets、model_pricing、query_management、storage_settings、nodes、domain_management、correlation、organization_management
- 各自对应后端 `routes/{cipher_keys,connectors,storage_providers,domains,...}.rs`

### P2 —— 剩余主页面二级路由 + 杂项

- Logs `searchJobInspector`、Metrics `promql-builder`、Traces `trace-details` / `session-details`、Dashboards `import` / `addPanel` / `panel-settings` / `scheduled`
- Stream Explorer 独立页 `/streams/:id` 深入
- Service Graph 独立页 `/service-graph`
- Alerts 子页：history、insights、import-semantic-groups、anomaly-detection
- Action Scripts `/actions`
- Short URL `/short/:id`
- Ingestion catalog 实页化（替换当前静态指南）：custom 子分类 logs/metrics/traces × {curl,fluentbit,fluentd,vector,filebeat,otel,logstash,syslog-ng,prometheus,otelcollector,telegraf,cloudwatch} + recommended {k8s,windows,linux,aws,gcp,azure,frontend-monitoring}

## Capabilities

### New Capabilities

- `web-rum`: RUM sessions / errors / performance / source maps
- `web-functions`: VRL function library + enrichment tables
- `web-pipeline-editor`: 可视化 pipeline 编辑器 + 历史 / 回填
- `web-iam`: 独立 IAM 路由组（users/groups/roles/service-accounts/quota/invitations/orgs management）
- `web-settings-admin`: Settings 下的 16 个子路由聚合
- `web-action-scripts`: 自定义 action 脚本管理
- `web-short-url`: 短链分享

### Modified Capabilities

- `web-shell`: Sidebar 新增 4 个顶层入口（RUM / Functions / Actions / IAM），Settings 下 sub-nav 扩到 16 项
- `web-shell-crud`: 主页面二级路由扩展（Logs/Metrics/Traces/Dashboards/Alerts/Streams 各加 2-4 个子路由）

## Impact

- **代码规模**：~50 个新路由文件，按 P0/P1/P2 分批；每批一个 follow-up sub-change。本 change 落 P0（≈15 个路由），后续 follow-up 落 P1 / P2。
- **后端依赖**：P0 绝大多数后端路由已就绪（rum / functions / sourcemaps / identity / rbac_policies）；只有 pipeline 的 history/backfill 子端点缺，列为 follow-up。
- **i18n**：所有新页文案走 `t()`，加 `i18n/{en,zh-CN}/{rum,functions,iam,actions,settings-admin}.json` 五个新命名空间。
- **a11y**：新增 a11y-routes spec 自动覆盖新路由 critical=0。
- **风险**：单 change 跨 50+ 路由极大；缓解是只在本 change 落 P0，P1/P2 作为后续 follow-up change 单独 propose / apply。
- **跟随**：land P0 后开 `web-feature-parity-settings` 落 P1；再开 `web-feature-parity-misc` 落 P2。
