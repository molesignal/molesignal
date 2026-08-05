## Context

MoleSignal 是一个 Rust 单体仓库、可按 `router / ingester / querier / compactor / alert_manager / standalone` 角色拆分部署。HTTP 使用 Axum，内部与外部 gRPC 使用 Tonic/Arrow Flight，持久化路径经过 PostgreSQL、WAL、Parquet 和 `object_store`。仓库已经依赖 `tracing`、OpenTelemetry SDK/OTLP，并正在完成 `ingest-self-telemetry` 变更：

- 全局 subscriber 已能输出 console/file、可选外部 OTLP，并将自身 logs/traces 放入有界队列。
- `_molesignal` 已是系统保留流名，自身遥测可通过可信内部 ingest 路径写回现有 typed stream。
- 当前业务代码几乎没有 Span，HTTP/gRPC 上下文没有统一提取/注入，异步任务、SQL 和对象存储也没有链路语义。
- 当前自身 Span 转换与公共 OTLP 接入只共享有限字段，无法完整保存 Links、标准 Events、Scope 和 dropped counts。
- 当前外部 OTLP 只支持一个 gRPC endpoint，采样、TLS/认证、日志关联、shutdown flush 和运行时重配能力不足。
- 当前组织权限完全基于 Membership；`root_email` 只用于引导与展示，没有平台级角色或系统作用域。
- 当前 License 是进程内全局对象，普通已登录用户可读、任意组织的 OrgAdmin 可上传，上传结果不持久化。

本变更以 `ingest-self-telemetry` 已完成为实施前提，并显式替换其中“可配置普通管理组织、功能默认关闭”的设计。当前仍处开发阶段，没有 `_sys` 组织和需要保留的 Trace 历史数据，因此不承担旧 Trace Schema 或旧 License API 的兼容成本。

## Goals / Non-Goals

**Goals:**

- 一次性交付所有后端模块的端到端 Trace，覆盖 HTTP、gRPC、Arrow Flight、出站网络、SQL、对象存储、写入、查询、Pipeline、告警、报表、AI、Compactor 和后台任务。
- 在跨角色、跨节点链路中保持 W3C Trace Context，并为长任务、重试、扇出与流式会话使用正确的 Parent/Link 语义。
- 通过一个分布式、容量可配置、永不阻塞业务的尾采样管线，保留全部已观察到的错误/慢链路并按比例保留普通链路。
- 让自身回灌和外部 OTLP 共享采样决定，但隔离队列、重试和故障。
- 完整、可查询地保留 OpenTelemetry Span 数据模型，并把日志、Trace 和 API 响应关联到同一个 `trace_id`。
- 建立不可变 `_sys` 系统组织、平台管理员和短期系统作用域，使系统遥测与实例 License 具有清晰的安全归属。
- 满足明确的隐私、基数、性能、故障降级、审计、测试和上线门禁。

**Non-Goals:**

- 不新增专用前端页面；现有组织切换和 Trace 页面通过后端返回的 `_sys` 系统作用域复用。
- 不为 Trace exporter 或尾采样未决缓存增加磁盘 WAL、副本复制或崩溃恢复。
- 不保证采样决策窗口之后才到达的错误 Span 能复活已丢弃 Trace。
- 不允许普通租户查看 MoleSignal 内部 Trace 或任何 License 信息。
- 不记录请求/响应正文、Prompt/LLM 输出、SQL 参数、凭据、完整对象键或其他敏感高基数内容。
- 不兼容旧开发期 Trace 行 Schema、旧 `/api/v1/license` 接口或普通组织下的自身遥测归属。

## Decisions

### 1. 使用统一 Trace Pipeline，而不是并列的两套 exporter

应用边界只创建一次 OpenTelemetry/tracing Span。完成后的 Span 先转换为一个完整的内部 `CanonicalSpan`，再进入 `TracePipeline`：

1. producer 进程做轻量字段规范化、隐私清洗、大小限制和非阻塞入队；
2. standalone 本地处理，拆分角色按 `trace_id` 一致性哈希把 Span 发给一个 sampler owner；
3. owner 按 trace 聚合并作一次尾采样决定；
4.保留的 CanonicalSpan 同时 fan-out 到自身回灌 sink 与外部 OTLP sink；
5. 两个 sink 使用独立有界队列、超时、重试和健康状态。

这样两端得到相同 Trace 集合，采样、去重和完整性逻辑只有一份。备选方案“self-ingest 和 tracing-opentelemetry 各自采样/导出”无法保证决策一致；“producer 本地尾采样”看不到跨节点完整 Trace。

producer 到 owner 的内部传输沿用集群认证和 bounded retry。没有可用 owner、队列溢出或传输超过最大年龄时丢弃并计量，绝不阻塞业务。原始 producer Resource 必须原样保留。

### 2. 统一 W3C 上下文与信任边界

HTTP、gRPC、Arrow Flight 和内部集群协议统一提取/注入：

- `traceparent`、`tracestate` 使用 W3C Trace Context；
- Baggage 默认只允许 `org.id` 和 `request.id`；
- `org.id` 永远由认证结果覆盖，非法 `request.id` 重新生成；
- Baggage 只向配置白名单中的内部目标透传；
- 调用第三方时只透传 Trace Context，剥离内部 Baggage；
- 暂不支持 B3/Jaeger header。

外部客户端携带的 sampled flag 只作为上下文输入，不能强制本地保留。尾采样需要本地记录完整候选 Span，因此入口会启动 deferred/recording 决策；只有可信内部身份或受限调试令牌能够标记强制保留。调试令牌最长一小时，必须限定组织、路由或条件，并单独限流、可撤销、全程审计。

无效或格式错误的上下文不影响请求：丢弃该上下文、创建新 Trace 并增加低基数安全指标。

### 3. 用语义化边界 Span，而不是给每个函数埋点

Span 命名和属性锁定一个 OpenTelemetry Semantic Conventions 版本；自定义字段使用 `molesignal.*`。首期一次性覆盖全部模块，但粒度遵循“边界全覆盖 + 核心阶段细化”：

- HTTP/gRPC/Flight 入站：以低基数路由模板或 RPC service/method 命名 `SERVER` Span；
- 出站 HTTP/gRPC/集群/联邦：创建 `CLIENT` Span并注入上下文；
- SQL：事务与逻辑查询 Span，记录 operation、collection/fingerprint、等待/行数/错误，不记录 SQL 参数；
- 对象存储：由统一 `ObjectStore` 装饰器覆盖 put/get/get_range/head/list/delete/copy/rename/multipart，记录 backend、bucket、对象类别、大小、缓存状态、重试和结果；
- 写入、查询、Pipeline、告警、通知、报表、AI、Compactor 和各 worker：在业务阶段创建 `INTERNAL / PRODUCER / CONSUMER` Span；
- 批量 ingest 按请求、批次和处理阶段创建 Span，禁止每条事件一个 Span；
- 缓存访问只在父 Span 记录 hit/miss；慢回源和错误才创建 Span/Event；
- 一次逻辑重试只有一个 Span，各 attempt 作为 Event；
- multipart 只有一个逻辑 Span，各 part 作为 Event 和聚合字段。

对象键只记录规范化类别/前缀和可选 HMAC 指纹，不记录完整 key。HTTP 只记录 route template，不记录 query/body；SQL 只记录 fingerprint；AI 只记录 provider/model/token 数/阶段/工具名，不记录内容；通知不记录收件人正文或 secret。

### 4. 明确定义异步和流式链路

- 请求生命周期内的短任务继承当前父 Context；
- 队列、延迟、重试任务持久化 Trace Context，但消费执行创建新 Trace 并用 Link 关联生产请求，避免把等待时间算入请求关键路径；
- 定时任务创建新 root；
- fan-out 子任务是同一父阶段下的并列 Span；
- SSE、流式 HTTP、流式 gRPC/Flight 将握手与会话分离；握手在响应建立后结束，会话默认每 30 秒或 1,000 条消息滚动一个 linked segment；
- 长任务与每次重试同样使用新 Trace + Link，确保尾采样窗口可控。

通用日志不会自动复制为 Span Event。普通 log 仅注入 `trace_id/span_id`；只有显式业务 checkpoint、retry、异常和流式 segment 事件进入 Span。

### 5. 内置分布式尾采样

同一 `trace_id` 经 rendezvous/一致性哈希进入同一 sampler owner。owner 使用有界内存表聚合 CanonicalSpan，并按以下顺序决策：

1. 可信强制保留；
2. 任一 Span 为 ERROR；
3. 任一 Span 超过其类型/路由慢阈值；
4. 命中平台配置的有序规则；
5. 按 `trace_id` 确定性哈希执行默认比例（生产默认 10%，开发/测试默认 100%）。

默认决策窗口 30 秒，可在 5–120 秒配置。root 结束后等待 1 秒 grace 即可提前决策；决策缓存保留足够时间让迟到 Span 复用相同结果。窗口结束后的迟到 Span不得复活已丢弃 Trace，需增加 late-span 指标。sampler owner 宕机时允许丢失一个窗口内的未决 Trace，不做副本或 WAL。

默认慢阈值：

| 类型 | 默认值 |
|---|---:|
| 普通 HTTP/gRPC | 1 s |
| 查询 | 5 s |
| 批量写入 | 2 s |
| 数据库 | 200 ms |
| 对象存储 | 500 ms |
| 外部调用 | 1 s |
| 后台任务 | 30 s |

任一子 Span 超阈值会保留整条 Trace。阈值可按路由/任务覆盖。

采样缓存达到容量或等待上限时，优先保留已发现错误/慢信号；普通 Trace按 `trace_id` 比例提前决策，其余丢弃。所有容量从配置的内存预算、最大 trace 数、最大 span 数和估算平均行大小计算，代码不写死部署规模。

### 6. CanonicalSpan 保存完整且有界的 OTLP 数据模型

公共 OTLP ingest、自身回灌和外部 exporter 共享同一个模型，至少包含：

- trace/span/parent IDs、flags、state、name、kind、start/end/duration、status；
- Resource 和 Instrumentation Scope 名称/版本/属性；
- Span attributes、Events、Links；
- dropped attributes/events/links counts；
- schema/semantic-convention version；
- sampling reason、partial/truncated 标记和原因。

默认每 Span 最多 128 个 attributes、128 个 Events、128 个 Links，单字符串 4 KiB；错误、状态和标准语义字段优先保留。单 Trace 默认最多 1,000 个 Span；超限时优先保留错误和最慢 Span，其余按操作类型汇总数量、总耗时与字节数，并标记 partial。

内部和公共 OTLP 的重复提交按 `(org_id, trace_id, span_id)` 在可配置窗口内去重；首次完整记录为准，字段冲突、重复和迟到分别计量。

开发阶段直接采用新 Schema；不编写旧 Trace 行转换或历史回填。

### 7. Resource 身份按实际执行角色生成有效服务节点

必需 Resource：

- `service.namespace = "molesignal"`
- `service.name`
- `service.version`
- 稳定 `service.instance.id`
- `deployment.environment.name`
- `node.id`
- `cluster.id`
- 可用时的 region/zone

单角色进程使用按角色区分的 service name。多角色/standalone 进程保持一致进程 Resource，并为每个 Span写规范化执行角色；Trace 规范化和服务图由执行角色计算 effective service name，不能生成 `router+querier` 组合节点。服务图按 `(trace_id, span_id)` 配对，跨 ingest 节点的数据通过 trace-affinity owner 聚合。

### 8. 外部 OTLP 是显式、安全、隔离的静态通道

配置显式选择 `grpc` 或 `http/protobuf`，默认 gRPC；不从 URL 猜协议。支持：

- endpoint、timeout、batch、queue、gzip；
- 自定义 metadata/header；
- TLS 自定义 CA；
- 可选 mTLS certificate/key；
- 仅通过环境变量或 secret reference 解析凭据。

协议、endpoint、TLS、认证和 Resource 身份是启动期静态配置；格式、安全错误必须使启动失败。运行时 collector 不可用则指数退避、有界重试、超限丢弃并告警，不影响业务，也不写本地磁盘队列。

自身回灌和外部 sink 隔离；任一失败不影响另一端。若外部 endpoint 指向同一 MoleSignal 集群且自身回灌开启，默认拒绝该通道，除非显式 `allow_self_export=true`，同时仍执行幂等去重。

优雅退出按以下顺序执行：

1. 停止新 Trace 和 tail-sampler 接收；
2. 完成可决策 Trace并 flush 两个 sink；
3. 最多等待默认 10 秒；
4. 记录未导出数量后继续已有 drain/shutdown。

### 9. Trace 过滤、运行策略和故障状态相互独立

`trace.filter` 与 `RUST_LOG / telemetry.log_level` 分离。Trace 动态策略持久化在 `_sys`，由 `/api/v1/system/telemetry` 管理，包括：

- 运行启停；
- 正常采样比例和有序规则；
- 各类型/路由慢阈值；
- 决策窗口、缓存/队列软上限和 Span 限制。

外部 exporter 网络/密钥配置仍来自部署配置。启停优先级：

1. 部署级强制关闭；
2. `_sys` 持久化运行策略；
3. 代码默认开启。

动态配置通过原子快照/ArcSwap 类结构切换；已在处理的 Trace 固定使用其创建时策略版本，避免一条 Trace 内决策漂移。Exporter 静态参数变更要求重启。

运行时 Trace 故障不改变 `/healthz` 或 `/readyz` 成功状态，只在详细健康信息中显示 degraded，并通过指标/告警暴露。显式非法配置仍 fail-fast。

### 10. 建立不可变 `_sys` 系统组织

启动始终幂等确保一个 `Organization { name: "_sys", slug: "_sys", system: true }` 存在，而不依赖 Trace 是否启用。`_sys`：

- 由代码常量固定，配置模型不提供 `telemetry.self_collect.org_slug` 或兼容别名；
- 不允许普通 Membership；
- 普通用户的组织列表、搜索和跨组织查询完全隐藏；
- 不允许任何身份或应用数据库账号改名、改 slug 或删除；
- 由领域校验、Repository guard 和 PostgreSQL trigger/constraint 三层保护。

`_molesignal` typed stream：

- 仅内部身份可创建和写入；
- 名称、org 归属和 system 标记不可修改，流不可删除；
- 允许平台配置更新各 signal 独立 retention 和容量策略；
- trace 默认开启时创建 `traces/_molesignal`，其他 signal 保持既有默认行为。

如果 `_sys` 或系统流因运行时存储故障无法准备，核心服务继续启动，相关系统能力标记 degraded；结构性配置冲突或保留资源被篡改则 fail-fast。

### 11. 平台管理员和短期 system_scope

新增持久化平台角色，不再把平台权限硬编码为邮箱。`root_email` 仅在首次引导时获得首个平台管理员身份。系统禁止删除或撤销最后一名平台管理员。

平台管理员在普通组织 token 下仍不能访问平台 API；必须通过现有组织切换入口切换到 `_sys`。后端无需 Membership 即可为平台管理员签发最长一小时的 `system_scope` JWT，并在平台管理员的组织列表中返回一个带 system 标记的 `_sys` 项。普通用户永远看不到或选择它。

平台管理员 assignment 只决定用户是否可以进入 `_sys`。具体平台角色由数据库
`iam_builtin_role_purposes` 的 `platform_administrator` 用途选择并物化到
`_sys` 的 `iam_roles`；默认种子为名为 `Owner` 的 `platform_owner`。选择响应
和 capability snapshot 从该角色行读取显示名，并从 `iam_role_permissions`
读取细粒度平台权限。JWT 仅携带身份、组织和 scope，不携带角色或权限。

平台权限细分：

- `SystemTelemetryRead`
- `SystemTelemetryManage`
- `LicenseRead`
- `LicenseWrite`
- `PlatformAdminManage`
- `TraceDebug`

system_scope 不授予通用 Organization/Stream 写权限，也不能为 `_sys` 创建 `ms_*` API token。现有 Trace 页面所需的 stream listing/query 能力只读映射到 `_sys/_molesignal`；self telemetry 数据写入仍只允许内部身份。所有平台级 API 统一位于 `/api/v1/system/*`。

### 12. License 变为 `_sys` 下的实例级不可变版本资源

License 仍是实例唯一，不变成租户 License。新增不可变 `license_versions` 和单一 active pointer：

- 保存原始签名包、解析摘要、创建者和时间；
- 每次读取/激活都重新验证签名和有效期；
- 历史版本不可编辑、不可删除；
- 平台管理员可重新激活仍有效的历史版本；
- 更新、激活和回滚采用事务并热更新 `LicenseHolder`；
- 启动优先加载 `_sys` active version；
- 环境变量/文件仅用于首次引导或显式开启的灾备回退；
- DB 中当前 License 损坏或验签失败时安全降级 Community 并高优先级告警，不阻止核心服务启动。

普通用户看不到任何 License 信息。旧 `/api/v1/license` 移除；非 system_scope 请求访问 License 路由统一返回 404。平台管理入口位于 `/api/v1/system/license` 及其版本/激活子资源。

### 13. 隐私、审计和自监控是强制门禁

集中 sanitizer 在 Span 入队前执行，且在 sink 前再次断言禁止字段。禁止内容包括 Authorization/cookie、secret、正文、query 参数、邮箱/姓名、SQL 参数、完整对象键、Prompt/回复和 Tool 参数/结果。只允许内部不透明 `org.id`、本地 `user.id/api_token.id` 等诊断 ID；后两者不进入 Baggage。

平台管理员授予/撤销、system_scope 切换、License 版本上传/激活、Trace 启停/规则变更、调试令牌签发/使用/撤销全部写审计。审计只记录操作者、范围和变更摘要，不记录 License 签名包、secret 或令牌正文。

Trace 子系统至少暴露 generated/accepted/sample-kept/sample-dropped/exported/export-failed、按原因丢弃、late/duplicate/conflict/partial、队列深度/容量、tail cache、decision latency 和 exporter latency 指标。默认告警：

- 持续 exporter 失败；
- 队列或 tail cache 超过 80%；
- 丢弃率持续超过 1%；
- `_sys`、License 或动态策略加载 degraded。

指标标签禁止 trace_id、org_id、object key、route raw path 等高基数字段。

### 14. 一次性交付，但通过强制开关和灰度控制发布

不按功能拆分阶段：所有模块、采样、系统作用域和 License 变更在一个变更中完成。代码默认 `trace.enabled=true`；正式上线时部署配置先设置强制关闭，只在少量实例移除强制关闭做灰度，满足门禁后再全量开启。

自动化门禁包括：

- HTTP/gRPC/Flight/异步/存储的 trace continuity；
- 全模块关键 Span 和角色服务图；
- 双 sink 同决策与故障隔离；
- 错误/慢/比例/规则采样；
- 去重、迟到、容量溢出和 owner 变化；
- 敏感字段缺失与属性上限；
- exporter/self-ingest 故障不影响业务；
- `_sys` 不可变、平台权限与系统切换；
- License 持久化、验签、历史、降级和 404；
- shutdown bounded flush；
- 默认采样下 CPU 增幅不超过 5%、P95 延迟增幅不超过 3%。

## Risks / Trade-offs

- **[尾采样要求记录并传输所有候选 Span]** → 非阻塞有界队列、内存预算、trace-affinity、span/trace 上限和基准门禁；容量不足时按明确优先级降级。
- **[sampler owner 宕机丢失未决 Trace]** → 接受最多一个决策窗口的数据损失，快速重哈希新 Trace并告警；不引入业务关键路径 WAL。
- **[跨节点时钟偏差破坏瀑布图]** → duration 使用本地单调时钟，保留原始时间戳并标记 clock-skew；要求部署时钟同步，不静默改写原始数据。
- **[完整数据模型和 Events/Links 增大存储]** → 每 Span/Trace 上限、字符串截断、正常流量 10% 采样、7 天独立 Trace retention。
- **[动态策略变化导致 Trace 内不一致]** → 每条 Trace 绑定策略版本，变更只影响后续新 Trace。
- **[系统组织成为高价值目标]** → 无 Membership、短期 system_scope、细粒度权限、三层不可变保护、404 隐藏和完整审计。
- **[默认开启造成升级资源突增]** → 正式部署先用最高优先级强制关闭，灰度验证后按需开放。
- **[同一 endpoint 形成自导出回路]** → 启动校验、显式 override、内部 suppression 和去重。
- **[平台角色或 License DB 不可用]** → 缓存最后有效权限/License快照；无法验证时拒绝平台操作、License 降级 Community，核心数据面继续服务。
- **[现有 `ingest-self-telemetry` 设计冲突]** → 本变更显式修改其默认组织、启停、事件复制和故障语义；实施前先完成或合并该变更的最后验证任务。

## Migration Plan

1. 完成并固定 `ingest-self-telemetry` 基线，记录其当前测试结果。
2. 新增 `_sys`、平台角色、系统 token、动态配置和 License 版本表迁移；当前无 `_sys`，无需数据搬迁。
3. 替换 Trace CanonicalSpan Schema；开发数据不迁移、不保留兼容读取。
4. 实现统一上下文、instrumentation、trace-affinity 和 tail sampler，再接入 self-ingest 与外部 sink。
5. 将 License API 移至 `/api/v1/system/*`，移除普通读路径并完成权限/审计测试。
6. 在所有模块一次性接入 Span，运行完整功能、隐私、容量和性能门禁。
7. 发布代码时保持部署级强制关闭；小规模实例移除强制关闭进行灰度。
8. 灰度稳定后通过 `_sys` 动态策略逐步调整采样比例和容量，再全量移除部署级强制关闭。

回滚优先使用部署级强制关闭，立即停止新 Trace，不影响业务路径；已创建的 `_sys`、平台角色和 License 版本保留。由于 `_sys/_molesignal` 永久不可删除，代码回滚不得尝试清理这些资源。若必须回滚到不认识系统表的旧二进制，先验证旧二进制会忽略新增表和 system 标记。

## Open Questions

无产品决策遗留。具体默认内存预算、batch size 和 decision-cache TTL 在实现阶段通过基准测试确定，但必须保持可配置并满足本设计中的性能与容量契约。
