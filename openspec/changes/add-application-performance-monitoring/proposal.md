## Why

MoleSignal 已具备 OTLP Trace、Service Graph、Logs、Metrics、Continuous Profiling 和 RUM，但现有体验仍以遥测信号为中心，缺少按应用、服务、事务、依赖和后端错误组织的性能视图。直接把 RUM 改名为 APM 会造成能力预期错位，因此需要新增真正的 APM 产品层，并让 APM 与 RUM 作为两个独立一级模块通过 Trace、服务和版本上下文关联。

## What Changes

- 新增 APM 投影管线，在 Trace 候选完成规范化、路由和去重后、尾采样丢弃前，派生不受采样偏差影响的分钟级服务、事务、依赖和错误聚合。
- 新增应用服务目录，以 `service.namespace`、`service.name`、`deployment.environment.name`、`service.version` 和实例/SDK Resource 属性维护服务身份、环境、版本与采集健康状态。
- 新增服务 RED 指标时间序列，以及按 HTTP route、RPC method、messaging operation 或低基数 Span name 聚合的 Transaction 性能分析。
- 新增数据库、缓存、消息系统、外部 HTTP/RPC 和服务间调用的依赖性能分析；所有目标名称必须经过低基数归一化和隐私清洗。
- 新增后端错误分组：从 ERROR Span 和 `exception.*` Event 派生稳定指纹、出现趋势、首次/最近发生时间、受影响服务/事务/版本及代表 Trace。
- 新增版本对比：按 `service.version` 展示首次出现时间，并比较新旧版本的吞吐、错误率和延迟变化。
- 新增 `/api/v1/apm/*` 专用查询 API，以及 `/apm` 应用性能入口、服务工作台、Transactions、Errors、Dependencies 和 Deployments 页面。
- 保持 `/rum/*` 为 RUM 的独立 canonical route，将 `/apm/user-experience/*` 降级为兼容重定向；RUM ingest API 和 `rum_*` stream 保持内部稳定。
- 将 Source Maps、SDK 接入、采样、隐私和回放配置归入 `/rum/settings/*`，避免分析导航与配置导航混排。
- 复用现有 `/traces`、`/profiles`、`/logs`、`/metrics` 和告警能力作为下钻目标，不复制已有的信号浏览器。
- 支持 Prometheus remote_write v1 原生 Exemplar，并提供 Prometheus 兼容的 `query_exemplars`，让 Metrics 图表中的 `trace_id` 直接下钻到 Trace。
- 第一版不新增专有语言 Agent、Synthetic Monitoring、SLO/Error Budget 管理、主机/Kubernetes 资产清单或自动根因分析；这些能力作为后续独立变更。

## Capabilities

### New Capabilities

- `apm`: 定义预采样 APM 投影、服务目录、RED 指标、Transactions、Dependencies、后端错误分组、版本对比、查询 API、保留与容量边界。
- `web-apm`: 定义 APM 概览、服务工作台、事务、依赖、错误和部署导航，以及跨 Trace、Logs、Metrics、Profiles、RUM 的下钻行为。

### Modified Capabilities

- `web-shell`: 在分析区并列提供 APM 与 RUM 一级入口，并在不破坏既有深链接的前提下保持两个产品的导航边界。
- `ingest-protocols`: remote_write 在普通 samples 之外接收、校验并隔离保存原生 Exemplars。
- `query`: 通过 PromQL selector 和 Prometheus HTTP API 查询有界、租户隔离的 Exemplars。

## Impact

- **后端领域与应用层**：新增 `src/domain/apm/`、`src/app/apm/`，并在 Trace candidate owner 上增加有界、非阻塞、去重后的 APM projection port。
- **基础设施**：新增 APM service catalog、分钟桶、error group/occurrence 数据表及 repository；增加迁移、保留清理、聚合 flush 和健康指标。
- **HTTP API**：新增 `/api/v1/apm/overview`、`/services`、`/services/{service}`、`/transactions`、`/dependencies`、`/errors`、`/errors/{fingerprint}` 和 `/versions/compare`。
- **Prometheus 兼容性**：remote_write 保留 `TimeSeries.exemplars`；新增 GET/POST `/api/v1/prometheus/api/v1/query_exemplars`，普通 PromQL sample 结果不受影响。
- **前端**：新增 `web/src/routes/apm/` 与 `web/src/api/apm.ts`；调整产品 IA、权限映射、面包屑、命令面板、快捷键和 i18n；复用现有 Trace、Topology、Profile 和 RUM 组件，同时保持 `/apm/*` 与 `/rum/*` 两套独立页面外壳。
- **兼容性**：不修改 OTLP、RUM ingest、现有 stream 名称和信号查询契约；旧 `/apm/user-experience/*` 路径重定向到 `/rum/*`，API 新增而非替换。
- **运维**：增加 APM projection queue、drop、cardinality overflow、flush latency、late span 和 storage failure 指标；投影失败不得阻塞 Trace ingest。
