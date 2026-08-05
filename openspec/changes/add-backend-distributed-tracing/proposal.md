## Why

MoleSignal 已具备基础 `tracing` subscriber、可选 OTLP exporter 和自身遥测回灌，但业务路径几乎没有 Span，上下文不会跨 HTTP、gRPC、异步任务和存储边界传播，也无法对分布式 Trace 做一致尾采样。需要一次性补齐端到端 Trace 能力，使写入、查询、Pipeline、告警、后台任务、数据库和对象存储都能通过同一 `trace_id` 定位，同时把实例级遥测与 License 管理收敛到不可变的系统作用域。

## What Changes

- 为所有后端入口、出站调用、数据库、对象存储、核心业务阶段、异步/定时任务及长连接增加 OpenTelemetry 语义化 Span，并采用 W3C Trace Context 与受限 Baggage 传播。
- 新增内置、分布式、容量可配置的尾采样：错误和慢链路全量保留，普通链路默认按 10% 一致采样；同一 `trace_id` 路由到同一采样分片。
- 自身回灌与外部 OTLP 使用同一采样决策、隔离队列；外部导出同时支持 gRPC 和 HTTP/protobuf、TLS/mTLS、认证 metadata 及非阻塞失败降级。
- 扩展统一 Trace 数据契约，保存完整 Span Events、Links、Instrumentation Scope、dropped counts、采样/完整性元数据，并为重试、流式会话和高扇出场景定义有界模型。
- 新增独立 Trace 过滤、动态采样/阈值策略、隐私与基数限制、自监控指标、审计、强制采样调试令牌和发布门禁。
- 新增固定系统组织 `_sys`；平台级管理员可通过短期 `system_scope` 切换并复用现有 Trace 页面查看 `_sys/_molesignal`，普通组织和普通 Membership 不可见。
- 将实例级 License 的持久化、版本历史、激活与平台管理员管理统一归属 `_sys`，移除普通用户的 License 可见性。
- **BREAKING**：Trace 默认开启并默认自身回灌到 `_sys/_molesignal`；部署可用最高优先级强制开关在正式上线时关闭。
- **BREAKING**：`_sys` 的 `name/slug` 与 `_molesignal` 数据流名称、组织归属和系统标记永久不可修改，组织和数据流永久不可删除；保护覆盖领域层、Repository 和数据库层。
- **BREAKING**：平台 API 统一迁移到 `/api/v1/system/*`；原通用 License 读写入口不保留兼容别名，非平台用户访问统一返回 `404`。
- **BREAKING**：当前仍处开发阶段，新的 Trace 行契约不兼容历史开发数据，不提供旧 Schema 迁移。

## Capabilities

### New Capabilities

- `distributed-tracing`: 端到端 Span、上下文传播、双通道导出、分布式尾采样、隐私/容量边界、长连接和异步语义及验收标准。
- `system-scope`: 不可变 `_sys` 系统组织、平台管理员、短期系统作用域、系统权限与 `/api/v1/system/*` 管理边界。

### Modified Capabilities

- `telemetry`: Trace 与日志过滤分离，完善 OTLP exporter、日志关联、生命周期、自监控和运行时策略。
- `traces`: 扩展完整 OpenTelemetry 数据契约、去重、Trace 完整性及按角色生成服务图。
- `self-telemetry-ingestion`: 将自身 Trace 默认写入自动创建的 `_sys/_molesignal`，调整默认启停、独立保留期、采样和故障语义。
- `identity`: 增加平台级管理员、`system_scope` JWT/AuthContext 与无 Membership 的 `_sys` 切换。
- `license`: 将唯一实例 License 持久化到 `_sys`，支持不可变版本历史和平台专属管理。
- `audit`: 审计平台管理员、License、Trace 策略与强制采样操作，且不记录密钥或令牌正文。
- `ingestion`: 强化 `_molesignal` 系统流的内部写入和不可变保护，并接纳新版 Trace 行契约。
- `cluster`: 对自身 Trace 使用 `trace_id` 一致性哈希路由，以支持分布式尾采样、去重和完整服务图。

## Impact

- **后端边界**：Axum/Tonic middleware、Arrow Flight、集群/联邦协议、HTTP clients、SQLx、`object_store`、Pipeline、查询、写入、告警、报表、AI/LLM、Compactor 及全部后台 worker。
- **遥测核心**：`src/shared/telemetry.rs`、`src/shared/self_telemetry.rs`、Trace 规范化/查询、配置、bootstrap、shutdown、metrics 与审计。
- **身份与系统管理**：组织/用户/AuthContext/JWT、平台角色持久化、`/api/v1/system/*`、系统保留资源保护和数据库迁移。
- **License**：从进程内热替换扩展为 `_sys` 下的签名包版本存储、启动加载、重新验签、激活和安全降级。
- **依赖与运维**：锁定 OpenTelemetry Semantic Conventions 版本；新增 OTLP HTTP、TLS/mTLS 和尾采样实现。所有容量、阈值、队列和等待窗口参数化，默认不引入本地磁盘 Trace Export 队列。
- **前端**：不新增页面；平台管理员通过现有组织切换与 Trace 页面进入 `_sys`。平台管理 API 的专用 UI 不在本次范围。
