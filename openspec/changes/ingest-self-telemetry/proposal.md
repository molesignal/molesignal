## Why

MoleSignal 目前只把进程指标暴露在 `/metrics`、把日志写到 console/file，并可选把 trace 发往外部 OTLP；平台本身发生故障时，仍需要额外的 Prometheus、日志采集器和 trace backend 才能排查。把服务自身的 logs、metrics、traces、profiles 回灌到统一的 `_molesignal` 系统流后，运维人员可以直接使用现有查询、关联和火焰图能力观察 MoleSignal 自身。

## What Changes

- 新增进程内 self-telemetry runtime，把服务自身的四类信号固定写入 `_sys` 下、同名但不同 `stream_type` 的 `logs/_molesignal`、`metrics/_molesignal`、`traces/_molesignal` 和 `profiles/_molesignal`。
- 周期快照全局 Prometheus registry；镜像结构化 `tracing` event/span；周期采集 CPU/heap profile，并复用现有 profile 规范化、object-store 归档和 metadata 流。
- 提供标准 pprof 风格的 CPU/heap 调试接口，并保留现有 `/api/v1/debug/profile/*` 路径作为兼容别名；CPU 实现不再返回占位 `503`。
- 新增 `telemetry.self_collect` 配置，包括总开关、Metrics 子开关、采集周期、批大小和有界队列容量；Logs 与 Profiles 随总开关启用，Trace 回灌同时受总开关和独立 Trace 捕获策略控制；目标组织由常量固定为 `_sys`，外部 OTLP、console/file 和 `/metrics` 继续独立工作。
- 对 self-ingest 写入增加来源标记、递归抑制、背压丢弃计数和优雅关闭 flush，确保内部写入不会再次生成待回灌遥测并形成反馈环。
- **BREAKING**：精确流名 `_molesignal` 变为系统保留名；公共 ingest、stream CRUD 和 pipeline target 不得创建、覆盖、删除或变换该流。

## Capabilities

### New Capabilities

- `self-telemetry-ingestion`: 四类服务自身遥测的采集、归属、批处理、内部写入、系统流保护、递归抑制、背压与生命周期语义。

### Modified Capabilities

- `telemetry`: 本地日志/trace subscriber 与 Prometheus registry 增加可选的内部镜像输出及 exporter 自监控指标。
- `profiling`: 补齐可用的 CPU/heap pprof 风格端点、并发限制、访问门禁和兼容路径。
- `continuous-profiling`: 服务自身 profile 可指定 `_molesignal` metadata 流并复用现有规范化、归档、查询与下载链路。
- `ingestion`: 引入内部遥测写入来源，并保护 `_molesignal` 不受公共写入、用户 pipeline、计费和配额路径影响；既有 schema 校验与敏感字段 masking 仍适用。

## Impact

- **后端**：`src/shared/telemetry.rs`、`src/shared/metrics.rs`、`src/api/http/routes/profiling.rs`、`src/api/http/routes/profiles.rs`、`src/app/ingestion`、`src/bootstrap/wire.rs`、`src/config/telemetry.rs`、stream/pipeline API 校验与 shutdown 生命周期。
- **数据模型**：不新增 `StreamType`；四类流复用现有 WAL/parquet/object-store 布局，profile blob 继续使用现有 pprof zstd 归档格式。
- **依赖**：CPU profile 需要引入或启用能够导出 pprof protobuf 的 Rust sampler；heap profile 继续以 jemalloc 能力为主并在不支持的平台明确降级。
- **运维**：self-ingest 会产生额外存储与 CPU 开销；通过总开关、Metrics 子开关、采样周期、队列上限、保留策略和可观测的 drop/error 指标控制。
