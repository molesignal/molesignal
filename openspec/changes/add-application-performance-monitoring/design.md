## Context

MoleSignal 当前已有标准 OTLP HTTP/gRPC Trace 接收、统一 `CanonicalSpan`、trace-affinity candidate routing、去重与分布式尾采样、Trace 查询/瀑布视图、Service Graph、Continuous Profiling、跨信号跳转和完整 RUM。数据底座足以支持 APM，但当前产品视角仍然是“浏览信号”：

- `/web/traces` 按 `trace_id` 聚合，不能回答某个 endpoint/Transaction 的长期吞吐、错误和延迟；
- `/web/topology` 从跨服务 edge 聚合 node，服务吞吐会受到调用边数量影响，跨桶 P95 取最大值，适合拓扑态势但不适合作为服务 RED 指标；
- `/services/:service` 只有当前窗口快照和上下游列表，没有性能趋势、Transactions、后端错误、版本和实例视角；
- ERROR Span 与 `exception.*` Event 已保存，但只有 RUM 前端错误提供 issue grouping；
- RUM、Trace、Profiles、Logs 和 Metrics 已经能够互相下钻，不应在 APM 中复制一套浏览器。

APM 指标还有一个关键约束：现有 Trace 使用错误/慢请求优先的尾采样。如果从最终保存的 Trace 计算 RED 指标，错误率和延迟会产生系统性偏差。因此 APM 必须在 trace-affinity owner 完成候选去重后、尾采样丢弃前观察唯一 Span。

## Goals / Non-Goals

**Goals:**

- 交付以服务为中心的 APM v1，包括服务目录、RED 趋势、Transactions、Dependencies、后端错误分组和版本对比。
- 使用预采样的唯一 CanonicalSpan 生成准确、可合并、低基数的 APM 指标。
- 保持 Trace ingest 热路径非阻塞，APM 降级不能拖垮业务遥测。
- 在组织、环境、版本和时间范围上提供一致的 API 与 UI 过滤语义。
- 让 APM 与 RUM 保持独立一级模块，同时通过 Trace、服务、环境、版本与会话上下文互相下钻。
- 复用现有 Trace、Logs、Metrics、Profiles、RUM、Alert 和 Investigation Stack。
- 明确容量、隐私、保留、幂等和数据质量边界。

**Non-Goals:**

- 不开发 MoleSignal 专有语言 Agent；继续使用 OpenTelemetry SDK/auto-instrumentation。
- 不新增 Synthetic Monitoring、SLO/Error Budget、主机/Kubernetes 资产管理或成本分析。
- 不在第一版提供错误负责人、状态流转、评论等 issue-management 工作流。
- 不保存原始 SQL、URL query、请求/响应正文或其他现有 Trace sanitizer 禁止的数据。
- 不对历史 Trace 做全量回填；APM 聚合从运行支持该能力的节点启动时开始。
- 不移除 `/traces`、`/logs`、`/metrics`、`/profiles` 或 RUM backend contract。

## Decisions

### 1. APM 是应用视角的派生产品层，不是新的遥测信号

APM 不新增 `StreamType::Apm`，也不把 RUM、Trace 或 Profile 数据复制成另一份原始事件。它保存从 CanonicalSpan 派生的有界服务实体和聚合事实，并把原始证据下钻到现有信号页面。

选择该方案是为了保持现有四类信号和查询引擎边界稳定。备选方案“把 RUM 菜单直接改名为 APM”无法满足后端性能预期；“把所有信号页面搬进 APM”会制造重复导航和组件分叉。

### 2. 在 candidate owner 去重后、尾采样决策前投影紧凑 APM fact

在 `TracePipeline` owner loop 增加可选 `ApmSpanProjector` port：

1. candidate 在现有边界完成规范化、sanitization、trace-affinity routing；
2. owner 从 candidate 提取只含 APM 允许维度的紧凑 `ApmSpanFact`；
3. `TailSampler::accept` 返回现有 `CandidateDisposition`；
4. `Accepted`、`LateKept`、`LateDropped` 的唯一 fact 非阻塞送入 APM projector；
5. `IdenticalDuplicate`、`ConflictingDuplicate` 不投影；
6. TailSampler 按原逻辑决定是否保存/外发完整 Trace。

紧凑 fact 在 candidate 被 move 进 sampler 前提取，避免复制完整 attributes/events。它只携带 service/resource identity、kind、event time、duration、status、低基数 protocol fields、规范化 exception 摘要和 trace/span reference。

备选方案“从已保存 Trace 定时扫描”会被尾采样偏置；“每个 producer 本地聚合”会在重试和跨节点路由下重复；“把 projector 写进 TailSampler”会让采样领域逻辑依赖 APM。

### 3. 服务、Transaction、Dependency 和 Error 使用独立且明确的分类语义

服务身份：

- namespace：`service.namespace`，缺失为 `default`；
- name：`service.name`，缺失为 `unknown_service`；
- environment：优先 `deployment.environment.name`，兼容 `deployment.environment`，缺失为 `unknown`；
- version：`service.version`，作为维度而不是 identity；
- instance：`service.instance.id`，仅用于近似活跃实例数，不作为聚合主键。

服务 RED 只由 SERVER/CONSUMER Span 贡献；kind 缺失时仅允许 parentless Span 作为兼容 fallback。这样服务调用多个下游时仍只计一个入口请求。

Transaction 名称依次取：

1. `http.request.method + http.route`；
2. `rpc.service + rpc.method`；
3. messaging operation + destination template/name；
4. 经过低基数校验的 Span name；
5. `__other__`。

Dependency 只由 CLIENT/PRODUCER Span 贡献，按 service、database、cache、messaging、external HTTP/RPC 分类。数据库只保存 system、namespace/collection、operation 和可选 HMAC fingerprint，不保存 statement/parameters。

Error fingerprint 输入为 service identity、environment、error/exception type、规范化 top application frame 和 Transaction name。message 仅作为经过 masking、截断的代表文本，不进入 fingerprint，以免请求 ID 造成分组爆炸。

### 4. 延迟使用固定可合并直方图，禁止合并预计算 percentile

每个聚合 bucket 保存：

- request/operation count；
- error count；
- duration sum/min/max；
- 固定边界 latency histogram counts；
- exemplars 的有界 trace/span IDs。

查询跨 bucket、owner 或 rollup 时先逐 bucket 相加 histogram counts，再计算 P50/P95/P99。固定边界沿用 APM 专用毫秒范围并保留 `+Inf`；边界配置是 schema version 的一部分，变更必须创建新版本，不能混合计算。

该方案比每桶保留最多 N 个 sample 更有界、可合并且易于幂等快照。仓库已有 Prometheus histogram/quantile 语义，可复用测试思路，不依赖最大 P95 近似。

### 5. PostgreSQL 保存有界 owner snapshot，近期分钟桶再压缩为小时 rollup

第一版采用 PostgreSQL aggregate tables，而不是把 APM 聚合重新写入普通 telemetry streams：

- `apm_services`：服务目录和采集元数据；
- `apm_service_buckets`；
- `apm_transaction_buckets`；
- `apm_dependency_buckets`；
- `apm_error_buckets`；
- `apm_error_groups`；
- `apm_error_samples`；
- 对应 `*_hourly` rollup tables。

分钟桶主键包含 `(org, dimensions..., bucket_at, owner_id)`。每个 owner 对开放 bucket 周期性写入“绝对快照”，并带单调 `snapshot_seq`；repository 只接受更高 sequence 并整体替换该 owner snapshot。查询跨 owner 求和，因此 flush retry 不会重复计数。

默认保留最近 48 小时分钟桶；关闭并超过迟到 grace 的分钟桶事务性合并到小时 rollup，默认保留 30 天。数值均可配置。服务目录和 error group 元数据不随分钟桶立即删除，但按组织删除/合规清理同步清除。

选择 PostgreSQL 是因为当前 Service Graph 已使用同类分钟聚合，且 owner snapshot 需要条件 upsert 和幂等 rollup。为避免把 PostgreSQL 变成高基数遥测仓库，Transaction/Dependency/Error 严格限额、表按时间分区、使用短热保留和小时 rollup。若基准显示目标规模下不可接受，再将闭合 rollup 导出到内部 Parquet stream；该迁移不改变 API。

### 6. Cardinality limiter 在 fact 入桶前生效

按 org/service/environment/hour维护有界 dimension registry，默认上限由配置提供并通过负载测试确定：

- service identity 超限：拒绝新 identity 并计量，不把不同服务合并；
- Transaction/Dependency 超限：映射到同类型 `__other__`；
- version/instance 超限：不再增加明细维度，但继续计入无版本服务总量；
- error group 超限：映射到按 service/Transaction 的 overflow error group。

所有 registry 在窗口过期后释放，不能随累计历史无限增长。API 返回 overflow count，UI 明确显示“部分维度已聚合”。

### 7. Dedicated APM API 负责一致查询，不让页面拼接任意 SQL

新增 `src/api/http/routes/apm/`，内部调用 `src/app/apm/query/`：

- `GET /apm/overview`
- `GET /apm/services`
- `GET /apm/services/{service}`
- `GET /apm/transactions`
- `GET /apm/dependencies`
- `GET /apm/errors`
- `GET /apm/errors/{fingerprint}`
- `GET /apm/versions/compare`
- `GET /apm/health`（需要相同读权限，仅返回租户投影健康摘要；实例级细节留在系统健康）

公共 query context 统一解析 org、from/to、namespace、service、environment、version 和 pagination。所有 service detail 子数据必须从同一个 context 生成，避免页面 KPI 与列表使用不同过滤条件。

读权限复用 `streams.query`；system scope 复用 `sys.telemetry.read`。所有实体 lookup 带 org predicate，跨组织按 404 隐藏。

响应包含：

- `range`、`resolution`；
- `last_complete_bucket_at`；
- `data_quality { partial, gaps[], overflow_dimensions[] }`；
- 稳定 cursor 和 sort metadata。

### 8. APM 与 RUM 使用独立 canonical routes，旧嵌套路由保留为兼容入口

新增 `web/src/routes/apm/` 专属目录，按职责拆分 Overview、Services、ServiceDetail、Transactions、Dependencies、Errors、Deployments、ApmLayout 和 model/formatting 文件。RUM 页面继续归属 `routes/rum/`，并使用独立页面外壳；两者不互相渲染对方的页面导航。

Canonical routes：

- `/apm/overview`
- `/apm/services`
- `/apm/services/:service`
- `/apm/transactions`
- `/apm/dependencies`
- `/apm/errors`
- `/apm/errors/:fingerprint`
- `/apm/deployments`

RUM canonical routes：

- `/rum/overview`
- `/rum/applications`
- `/rum/sessions`
- `/rum/pages`
- `/rum/errors`
- `/rum/performance/*`
- `/rum/session-replay`
- `/rum/settings/*`

兼容策略：

- `/apm` 重定向 `/apm/overview`；
- `/services` 和 `/services/:service` 重定向到 APM canonical route 并保留 query；
- `/apm/versions/compare` 重定向到 `/apm/deployments` 并保留 query；
- `/apm/user-experience/*` 重定向到 `/rum/*`，保留 path params/query；
- `/rum/source-maps` 与 `/rum/upload-source-maps` 重定向到 `/rum/settings/source-maps*`；
- `/traces`、`/profiles`、`/logs`、`/metrics` 保持独立 canonical signal routes。

顶层 Sidebar 在分析分组中并列展示 APM 与 RUM。APM 内页使用“概览、服务、事务、调用链、依赖、错误、部署”sub-nav；RUM 使用“概览、应用、会话、页面、错误、性能、会话回放”sub-nav，Source Maps 仅出现在 RUM 设置导航中。

版本对比仍复用 `/api/v1/apm/versions/compare`，但作为部署分析能力出现在 `/apm/deployments` 与服务详情的部署入口中，不再占用“版本对比”一级标签。

### 9. Cross-signal drilldown 复用现有 handle 与时间上下文

APM API 返回低基数 filter handles 和有界 exemplars。前端通过现有 `SignalReference`、URL query 和 Investigation Stack 构建：

- service/Transaction/error → Trace search；
- Trace exemplar → Trace detail；
- service/trace → Logs；
- service + time → Metrics；
- service/trace/span → Profiles；
- RUM session/action → backend Trace；
- version comparison → baseline/candidate Profiles 或 Traces。

APM 页面不在客户端复制 SQL 模板来重算业务指标；只有跳转到通用 explorer 时生成已有查询语法。

### 10. 数据质量与健康是 API/UI 的一等状态

APM projector 维护 accepted、duplicate skipped、late accepted/dropped、queue full、cardinality overflow、flush failed、rollup failed、bucket lag 和 API latency 指标。若某时间段发生 projection drop，API 通过 gap ledger 标记 partial，直到该范围不再被查询或被明确修复。

APM 降级默认不影响 `/readyz`，但详细 health 和默认平台告警可见。UI 不用“0”掩盖缺失：完整空数据、部分数据和查询失败是不同状态。

### 11. 首次发布不回填历史 Trace

APM 表迁移增加 `apm_projection_started_at`。支持 APM 的节点启动后从新到达的 candidate 开始投影；UI 时间范围早于 started_at 时显示 activation boundary。历史 retained Trace 因尾采样偏置不能用于正式 RED 回填。

备选方案“历史回填并标记 approximate”容易让版本和错误率比较混入不同统计口径，因此第一版不采用。

### 12. Prometheus Exemplar 使用原生 remote_write 与 query_exemplars 契约

Prometheus remote_write v1 的 `TimeSeries.exemplars` 不转换为普通 metric sample，也不复制到
APM PostgreSQL 聚合表。接收器在完整 request preflight 中校验 Exemplar label 数量、名称、
值长度、重复名称及有限数值，然后将其写成同一 metric stream 的旁路行：

- 保留原 series labels；
- 使用内部保留 marker、Exemplar value 和 JSON labels 字段；
- 不写普通 PromQL 使用的 `value` 字段；
- Exemplar timestamp 与 sample 一样在协议边界从毫秒归一为微秒。

因此现有 PromQL evaluator 会自然跳过 Exemplar 行，且 Exemplar 与 metric stream 共享租户、
保留、WAL、Parquet 和 file-meta 生命周期。公共 remote_write label 不能覆盖这些内部字段。

查询提供 Prometheus 兼容的
`GET|POST /api/v1/prometheus/api/v1/query_exemplars`。查询引擎解析完整 PromQL 表达式，
收集其中的 vector/matrix selectors，按 metric、时间范围和 label matcher 读取旁路行；
remote_write 重试产生的相同 Exemplar 在响应前去重。单次查询最多返回 10,000 个唯一
Exemplar，达到上限时成功响应携带 warning，避免无界物化。

Metrics 图表在相同时间轴下展示 Exemplar 菱形标记；带 `trace_id` 的标记直接打开现有
Trace detail，没有 Trace ID 的 Exemplar 仍可见但不制造失效链接。APM 自身的预采样
`TraceExemplar` 继续服务于服务 RED 证据，两类 Exemplar 不共用存储语义。

## Risks / Trade-offs

- **[Candidate routing 或 projection queue 丢失会使 APM 指标不完整]** → 投影与 Trace 共用 owner 路由，增加 gap ledger、drop 指标、partial API 状态和容量基准；不以阻塞 ingest 换取完整性。
- **[PostgreSQL 分钟聚合在高服务/Transaction 基数下膨胀]** → 时间分区、owner snapshot、短分钟保留、小时 rollup、严格 cardinality limit 和基准门禁；保留未来把闭合 rollup迁到 Parquet 的接口边界。
- **[服务指标与旧 Topology node 指标数值不同]** → 明确语义：APM 服务吞吐来自 SERVER/CONSUMER，Topology 继续表示 edge traffic；Services 页面切换到 APM API并在测试中验证不重复计数。
- **[错误 message/stack 可能包含敏感或高基数数据]** → 先走集中 sanitizer/masking，再做规范化和长度/帧数限制；fingerprint 不含 message；API/日志禁止输出被移除字段。
- **[缺失或错误的 OTel semantic attributes 导致分类不准]** → 提供稳定 fallback、Instrumentation Health 提示和 `__other__`，不猜测 raw URL/SQL。
- **[旧 RUM/Services 深链接迁移破坏书签或报告]** → 路由别名/重定向保留 params/query，后端和 stream contract 不改，并添加覆盖全部旧路径的 Playwright 测试。
- **[新版本流量过少造成虚假回归判断]** → API返回 sample counts 和 insufficient-data，不在后端或 UI 中把低样本差异标为回归。
- **[首次部署没有历史趋势]** → 暴露 projection_started_at 和 activation boundary，明确不把 sampled Trace 回填成精确指标。

## Migration Plan

1. 添加 PostgreSQL 表、索引、分区/rollup 元数据和配置；APM 不提供启停或 kill switch，具备 Trace candidate owner 或 alert-manager rollup 职责的节点自动启动对应 worker。
2. 接入 `ApmSpanFact`、candidate disposition gate、projector queue、owner snapshot repository 和健康指标，完成去重、尾采样无偏、幂等及基准测试。
3. 先向少量代表性实例滚动部署，对合成固定流量校验服务请求数、错误率、histogram percentile、overflow 和 gap 语义，再继续扩大部署。
4. 启用 APM API，完成 tenant isolation、权限、分页、过滤、数据质量和保留测试。
5. 上线 `/apm/*` 页面并保持 Sidebar 原入口不变，通过直链和内部用户验证。
6. 在 Sidebar 并列启用 APM 与 RUM，启用 `/apm/user-experience/*`、`/apm/versions/compare`、`/services*` 兼容重定向并更新命令面板、快捷键、文档和截图。
7. 观察一个完整热保留周期后开启分钟到小时 rollup 清理，确认 PostgreSQL 行数和查询延迟门禁。

回滚时停止继续发布并通过标准部署机制恢复上一版本二进制；不提供独立 APM 配置开关。Trace/RUM ingest 与现有 signal explorers不受影响。新增表和已聚合数据保留以便恢复，不在二进制回滚中删除。旧 `/apm/user-experience/*`、`/apm/versions/compare` 与 `/services*` 路径始终有兼容处理。

## Capacity Gate And Accepted Defaults

2026-07-30 先运行 `node scripts/apm_capacity_spike.mjs`，把候选 owner 数、维度限额、分钟活跃率、保留期和 PostgreSQL 行/索引开销转换为明确门禁；随后用 PostgreSQL 17.10 和 release profile 运行 `APM_BENCH_DATABASE_URL=... cargo test --release --test perf_apm -- --nocapture`，完成真实 upsert、查询、四类 bucket 行宽、rollup 和幂等重试测量。

对现有代码路径的 focused spike 结论如下：

| 方案 | Owner snapshot 幂等 | 小范围租户查询 | Rollup/retention | 结论 |
|---|---|---|---|---|
| PostgreSQL aggregate tables | `ON CONFLICT ... WHERE snapshot_seq < EXCLUDED.snapshot_seq` 可原子替换 | tenant-leading B-tree 可直接覆盖服务和时间过滤 | 同一事务内合并、推进 completion marker、删除源分钟桶 | **接受为 v1 默认** |
| Internal WAL/Parquet stream | 只有 append；重试需在查询时对每个 key 做 `arg_max(snapshot_seq)` 去重 | 需要 file-meta 裁剪、对象读取和 DataFusion 聚合 | 适合闭合不可变数据，但无法原子替换开放 owner snapshot | 保留为超过 PostgreSQL 门禁后的闭合 rollup 导出方案 |

目标与默认值：

- 单个 candidate-owner 持续处理 `20,000 spans/s`，允许 `50,000 spans/s` 持续 30 秒的短时突发；默认 projector queue 为 `65,536`，生产者只能 `try_send`。
- 每组织每小时最多 `200` 个 service identity；每 service/environment 每小时最多 `32` 个 Transaction、`16` 个 Dependency、`16` 个 error group、`16` 个 version 和 `256` 个 instance。service 超限拒绝；Transaction/Dependency 折叠到 `__other__`；error 折叠到 overflow group；version/instance 停止增加明细。
- histogram schema `v1` 使用毫秒上界 `[1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1000, 2000, 4000, 8000, 16000, 30000, 60000, +Inf]`。schema version 是持久化主键与查询兼容性的一部分。
- flush interval 默认 `5s`，每次最多 `10,000` 个 owner snapshots；late grace `5m`；分钟热保留 `24h`；小时 rollup 保留 `30d`；每个 bucket 最多 `3` 个 exemplar，每个 error group 最多 `8` 个代表 sample。
- 版本比较两侧最少各 `1,000` 个 service request；低于阈值只返回 sample count 和 `insufficient_data`。
- 4 个 owner、5% 分钟活跃维度的重租户模型产生 `3,360 rows/min`、`4,838,400` 个热行和 `9,360,000` 个 30 天 rollup 行。数据库实测后向上取整的含索引行宽为 service `1,030 B`、Transaction `1,110 B`、Dependency `1,110 B`、error `1,220 B`，确定性模型为 `14.91 GiB`；门禁为每重租户不超过 `11,000,000` 个热行和 `16 GiB` 估算总量。
- `dimension_key` 持久化为 32-byte `BYTEA`，而不是 64 字符 hex；哈希与幂等键语义不变，同时避免 heap 和主键重复存储 hex 膨胀。真实四类加权外推由 `16.273 GiB` 降到 `14.852 GiB`，因此不需要牺牲 30 天查询范围。

性能门禁：

- fact 提取加内存聚合 `p99 <= 25µs/span`，单次非阻塞 enqueue `p99 <= 5µs`；目标负载下 projection queue drop `< 0.1%`。
- projector 稳态内存 `<= 512 MiB/owner`；`10,000` snapshots 的 PostgreSQL flush `p95 <= 500ms`。
- 24 小时范围的 overview/service list 查询 `p95 <= 500ms`，30 天 service detail/version compare 查询 `p95 <= 750ms`。
- 单组织一个小时的 rollup 加 retention 事务 `p95 <= 60s`，且重试后的 count/histogram 与单次执行逐位相同。

最终 release 基准结果：

| 路径 | 数据集 | 实测 | 门禁 |
|---|---:|---:|---:|
| 内存聚合 | 100,000 facts | p99 `2.041µs` | `25µs` |
| 非阻塞 enqueue | 50,000 facts | p99 `1.500µs`，drop `0%` | `5µs`，drop `<0.1%` |
| PostgreSQL snapshot flush | 10,000 rows，20 samples | p95 `452.343ms` | `500ms` |
| Overview query | 10,000 owner rows → 2,500 minute rows，20 samples | p95 `51.762ms` | `500ms` |
| 四类存储外推 | 24h hot + 30d rollup | `14.852 GiB` | `16 GiB` |
| 小时 rollup | 40,000 source rows → 3,250 rows | `621.681ms`，retry source `0` | `60s`，重试幂等 |

全量重写 10,000 个开放 snapshot 的 20-sample 压力循环会在 autovacuum 前把 service relation 已分配空间从 `10,256,384` 增至 `167,845,888` bytes。这个值不作为全部保留行的 live-row 外推：生产只反复替换当前开放分钟，闭合分钟不再更新；但分阶段发布及稳态运行必须持续监控 dead tuples、relation/index size 和 autovacuum 追赶能力。

若后续目标环境在同一数据集上超过任一硬门禁，必须停止扩大部署并回滚受影响版本；优先在修正版中降低可配置限额/保留期。若小时 rollup 仍超过容量或查询门禁，则只把**闭合且已完成**的 hourly rollup 导出到内部 Parquet stream，开放分钟 snapshot 和 API contract 保持不变。
