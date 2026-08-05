## Why

平台已具备 logs / metrics / traces / RUM 四类遥测信号，但缺少**持续性能分析（Continuous Profiling）**——定位 CPU、内存分配、锁竞争等"代码级"资源消耗的信号。排障时运维与开发者只能下钻到服务 / 请求层，无法回答"具体哪一段代码在烧 CPU、吃内存"。火焰图（flame graph）是该领域的事实标准可视化，而 `web/src/product/ia.ts` 的遥测支柱中恰好独缺这一环。

本变更把持续性能分析作为**新的一类遥测流（`StreamType::Profiles`）**纳入平台，复用既有的摄取 / 存储 / 查询 / 配额 / 多租户基础设施，提供火焰图浏览、差分对比与 trace 关联的端到端体验。

> 注意：本能力与现有 `profiling` 能力（`/debug/profile/{cpu,heap}`，仅用于诊断本服务自身）相互独立、互不替代——一个面向**被观测的用户应用**，一个面向**本服务运维**。

## What Changes

- **新增 `StreamType::Profiles`**：在 proto、domain、前端三处枚举同步扩展，使统一摄取管线接纳 profiles 批次。
- **三种摄取入口**，全部规范化到统一内部表示后再落盘：
  - OTLP Profiles 信号（OpenTelemetry 第四信号，复用现有 OTLP 摄取链路与 collector）
  - Pyroscope 兼容 `/ingest`（`format=pprof|folded|lines`，Java 走 JFR）
  - pprof / JFR 文件直传
- **存储**：原始 profile 以规范 pprof 形式 zstd 归档至 object store（保真、可被外部 pprof 工具消费），元数据行进 parquet 流（service / profile_type / labels / trace_id / object_key / 聚合指标），复用既有保留与配额。
- **查询与聚合**：服务端将时间窗口内匹配的 profile 合并为火焰图（flamebearer 结构），支持差分（diff）与 top-functions；元数据检索走现有查询引擎。
- **trace 关联**：profile 携带 `trace_id` / `span_id` 时，traces 详情可下钻"该 span 期间的火焰图"，profiles 可反向跳转对应 trace。
- **前端新增 Profiles 模块**：IA 注册、火焰图浏览器、列表 / 筛选、diff / 对比、关联导航、空状态与接入引导、en + zh-CN 文案。
- **门禁与配额**：核心能力 OSS 可用；差分、跨服务聚合、长期保留、服务端符号化等增强按版本（edition）门禁；摄取（含归档字节）计入既有 per-org 配额。

## Capabilities

### New Capabilities

- `continuous-profiling`: profiles 信号的摄取协议矩阵、规范化解析、归档 + 元数据存储模型、火焰图 / diff 聚合查询、trace 关联、保留与配额。
- `web-profiles`: Profiles 前端模块——IA、火焰图浏览器、列表 / 筛选、差分对比、关联导航、空状态与接入引导、i18n。

### Modified Capabilities

- `ingestion`: 统一摄取管线接纳 `StreamType::Profiles`（schema-on-write、pipeline / masking 适用性、drain 语义）。
- `storage`: profiles 的 object-store key 规范与 parquet 元数据落盘布局、保留。
- `quotas`: profiles 摄取（含归档字节）纳入 per-org 配额计量。

## Impact

- **受影响代码**：`proto/ingest/v1/ingest.proto`、`crates/domain/src/stream`、`crates/domain/src/ingestion`、`crates/app/src/ingestion`、`crates/api/src/http/routes/*`（新增 profiles / pyroscope / otlp-profiles 入口）、`crates/infra`（解析、归档、聚合）、`web/src/product/ia.ts`、`web/src/routes/*`、`web/src/api/*`、`web/src/viz/*`、`web/src/i18n/**`。
- **新增依赖（待评估）**：pprof protobuf 解析（prost 由 `profile.proto` 生成）、JFR 解析（Java 来源，本变更内交付）、zstd（已具备）、OTLP Profiles proto（Alpha，需钉选版本）。
- **风险**：OTLP Profiles 处于 Alpha 且有 breaking change —— 以适配层隔离、钉选 proto 版本、UI 标注 experimental；火焰图聚合需读多个 blob —— 设窗口内 profile 数上限 + 结果缓存；高基数堆栈存储成本 —— zstd + 保留策略 + 采样。
- **验证**：当前 CI 不可用，合并前在本地执行全量 `cargo test` / `cargo clippy` 与 `pnpm -C web typecheck` / `lint` / `test`。
