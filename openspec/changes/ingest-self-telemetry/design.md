## Context

MoleSignal 已有四类可观测数据的统一存储能力，但服务自身的遥测仍分散在进程外：

- `src/shared/telemetry.rs` 在 `main()` 很早阶段安装全局 `tracing_subscriber`，日志只写 console/file，span 只能可选导出到外部 OTLP。
- `src/shared/metrics.rs` 保存全局 Prometheus registry，`/metrics` 只提供文本 scrape。
- `IngestService` 负责 schema-on-write、pipeline、masking、service graph 与 `IngestSink`，最终进入 WAL/parquet；它目前没有可信内部来源的概念。
- continuous profiling 已支持 pprof 规范化、object-store 归档和 `StreamType::Profiles` metadata，但公共入口固定使用 `default` profiles 流。
- 自诊断 profiling 已有 `/api/v1/debug/profile/{cpu,heap}` 骨架；CPU 仍返回占位 `503`，heap 返回 jemalloc 原生 dump，且非 standalone 角色没有一致的节点级接口。

本变更跨越 subscriber、metrics registry、ingestion、cluster routing、profiling、bootstrap 与 shutdown。最重要的约束是：采集和写入路径本身会产生 logs/metrics/traces，必须从架构上阻断反馈环；tracing callback 也不能等待数据库或网络。

## Goals / Non-Goals

**Goals:**

- 将 MoleSignal 自身 logs、metrics、traces、profiles 写入不可变 `_sys` 下四个 typed `_molesignal` 流。
- 保留现有 console/file、外部 OTLP 和 `/metrics` 行为。
- 在 standalone 与拆分角色部署中都能收集每个节点的数据，并保留原始节点身份。
- 提供真实、受保护、节点级的 pprof 风格 CPU/heap 接口。
- 对队列、递归、存储开销、profile 开销、启动与关闭顺序给出明确边界。

**Non-Goals:**

- 不复制 self telemetry 到每个租户，也不向普通租户授予跨组织查看权限。
- 不让公共客户端写入或管理 `_molesignal`。
- 不实现完整的 Go `net/http/pprof` handler 集合（如 goroutine、mutex、block）；本次只提供对 Rust 服务有意义的 CPU 与 heap。
- 不保证进程崩溃时内存队列中的最后一批数据可恢复；正常关闭提供 bounded flush。
- 不替代外部 collector/exporter；内部回灌与现有出口可并行启用。

## Decisions

### 1. 使用固定 `_sys` 和四个同名 typed stream

目标组织由代码常量固定为 `_sys`，配置模型不提供 `telemetry.self_collect.org_slug` 或兼容别名；功能整体默认关闭。启用后在 system identity bootstrap 完成后预创建：

- `(_sys, "_molesignal", Logs)`
- `(_sys, "_molesignal", Metrics)`
- `(_sys, "_molesignal", Traces)`
- `(_sys, "_molesignal", Profiles)`（仅 profiles 开启时）

四类流同名但由现有 `(org_id, stream_name, stream_type)` 键隔离，满足用户提出的 `_molesignal` 数据流，同时无需新增 `StreamType` 或特殊物理存储。默认 retention 为 7 天，可独立配置。

不支持把目标切换到普通租户，也不把数据复制给所有 org。`_sys` 的平台管理员和 system-scope 语义负责系统遥测的归属与授权。

### 2. 在应用边界增加不可伪造的内部来源

不把 `origin` 字段加入可由外部反序列化的 `IngestBatch`。`IngestService` 增加 crate-private 的 `ingest_internal(batch, InternalIngestKind::SelfTelemetry)`，公共 `ingest(batch)` 永远是 external。远程角色使用独立的、集群认证的内部 RPC/metadata，入口在验签后才能调用 internal 方法。

internal self-telemetry 写入：

- 允许精确目标 `_molesignal`；
- 继续 schema 校验/演化、masking、WAL durability 和 drain gate；
- 跳过 user pipeline（系统流不可配置 pipeline）；
- 不经过 HTTP/gRPC public adapter 上的 billing/quota gate；
- 在 suppression scope 内执行。

精确名称 `_molesignal` 在所有公共 ingest adapter、stream mutation API 和 pipeline validation 处统一拒绝；查询仍走正常 org authorization。只保留这个精确名称，不扩大成 `_` 前缀规则。

### 3. subscriber 使用 late-bound、非阻塞的双出口

`init_full` 安装现有 formatter、可选 external OTLP layer，以及一个 `SelfTelemetryLayer`/内部 span exporter。内部 hook 只做字段转换的轻量部分和 `try_send`，不得执行 async/blocking I/O。`TelemetryGuard` 持有：

- logs、traces 各自的 bounded channel；
- 可 late-bind 的 runtime activation handle；
- 外部 tracer provider 与 non-blocking writer guard；
- shutdown/flush handle。

subscriber 在 bootstrap 前安装，因此能够保留有限的启动 logs/spans；功能关闭时 hook 为无 worker 的快速 no-op。采用独立 signal queue，避免 log storm 饿死 traces 或 lifecycle control。

不采用“把 console JSON 文件再采回来”或“loopback 发送到自身 OTLP HTTP”方案：二者分别丢失 span 生命周期信息、引入额外解析/网络，并显著放大递归风险。

### 4. metrics 直接读取 `MetricFamily`，不 scrape 自己

`shared::metrics` 增加 structured snapshot API，直接遍历 registry 的 `MetricFamily`：

- counter/gauge：每个 label set 一条 sample；
- histogram：输出 `<name>_bucket`（含 `le`）、`<name>_count`、`<name>_sum`；
- summary：输出 quantile（含 `quantile`）、count、sum；
- 每条写入 `metric_name`、`metric_kind`、`value`、原 labels、resource identity。

所有样本进入一个 `metrics/_molesignal` 流，而不是按 metric name 建数百个流；这与用户指定的系统流一致，并可用 SQL/metrics explorer 按 `metric_name` 筛选。`/metrics` 的 Prometheus 文本输出保持不变。

### 5. logs 与 traces 共用 tracing 上下文进行关联

log layer 将 `tracing::Event` 规范化为 `RawEvent`，保留 level、target、message、结构化字段、source location、thread 和当前 trace/span ID。span 通过 OpenTelemetry `SpanData`（或等价内部 finished-span 表示）转换为当前 OTLP ingest 使用的 trace 字段；实现时把 HTTP OTLP handler 中可复用的转换逻辑下沉为共享 adapter，避免两套 schema 漂移。

同一 resource 实例在进程启动时生成一次：`service.name=molesignal`、版本、角色集、node ID、process-lifetime instance ID。

### 6. profile 捕获复用 continuous-profiling 存储

CPU sampler 选用可导出 `perftools.profiles.Profile` 的 Rust pprof 实现，并通过一个进程级 semaphore 保证 scheduled capture 与 HTTP capture 不重叠。scheduled profiles 独立开关默认关闭；启用后默认每 10 分钟采 10 秒 CPU。heap 通过当前 jemalloc profiler 获取样本，再由 allocator adapter 转成 `NormalizedProfile` 和 canonical pprof；不支持的平台显式标记 unavailable。

`store_profile` 被拆为可指定 trusted metadata stream 的服务：

- public upload/Pyroscope/OTLP 仍固定写 `profiles/default`；
- self capture 固定写 `profiles/_molesignal`；
- profile blob 继续走现有 `profiles/<org>/<service>/<type>/<date>/<id>.pprof.zst`；
- pprof HTTP capture 在响应完成后异步归档，归档失败只计数/告警，不破坏已完成的下载。

### 7. pprof 使用独立的节点级 profiling listener

启用 `[profiling]` 后，每个 role 都启动一个轻量 listener，默认 `127.0.0.1:5084`，提供：

- `GET /debug/pprof/profile?seconds=N`
- `GET /debug/pprof/heap`

现有 `/api/v1/debug/profile/cpu` 与 `/api/v1/debug/profile/heap` 调同一个 capture service，作为兼容别名。listener 默认不远程暴露；`allow_remote=true` 时仍要求 Administrator token。CPU duration 限制为 1–120 秒；并发 capture 返回 `409 + Retry-After`。不支持的 heap 返回 `501`，不再用成功响应或模糊 `503` 伪装 profile。

独立 listener 比只挂主 HTTP router 更适合多角色部署：纯 ingester/querier/compactor 节点也可被节点本地运维工具抓取，且可以单独通过 bind/network policy 隔离。

### 8. 双重递归抑制和明确背压

采用两层防线：

1. self-telemetry worker、remote routing、profile archive 在 task-local/thread-local suppression scope 内执行；layer/exporter 看到 suppression 标志立即跳过。
2. self-telemetry 模块自身的内部诊断 target 进入 denylist，防止跨 task 的 retry/queue diagnostics 被再次采集。

callback 只 `try_send`。每个 signal 有独立 queue capacity、batch max events 和 max delay。满队列直接 drop，不阻塞业务请求；通过 `self_telemetry_dropped_total{signal,reason}` 等低基数指标暴露。metrics snapshot 可以包含这些 exporter 指标；它只按固定 interval 生成下一次样本，不形成事件级递归。

### 9. role-aware sink 与关闭顺序

standalone/ingester 走本地 `ingest_internal`。router/querier/compactor/alert-manager 节点用 cluster registry 选择 ingester，并经集群认证的 ingest RPC 发送；失败按 bounded exponential backoff 重试，超过 queue/age 限制后 drop。事件上的 resource identity 始终是 producer，而不是接收 ingester。

正常关闭顺序调整为：

1. 停 metrics/profile producer，并停止接收新的 internal events/spans；
2. 在配置 timeout 内 flush signal queues；
3. 调用 `DrainController::begin_drain()`；
4. 沿用现有 WAL drain/flush 和 server shutdown。

没有 ingester或 flush timeout 不能阻塞进程退出。崩溃时未出队数据允许丢失，已到 WAL 的数据仍由现有 replay 保证。

### 10. 配置形态与默认值

建议配置：

```toml
[telemetry.self_collect]
enabled = false
retention_days = 7
metrics_enabled = true
metrics_interval_secs = 15
queue_capacity = 8192
batch_max_events = 256
batch_max_delay_ms = 1000
flush_timeout_secs = 5
profile_kinds = ["cpu"]
profile_interval_secs = 600
profile_duration_secs = 10

[profiling]
enabled = false
bind = "127.0.0.1"
port = 5084
allow_remote = false
```

配置解析 SHALL 校验 interval、duration、queue 和 batch 非零以及 CPU duration 不超过 120 秒。现有 `MS_PROFILING_ENABLED` / `MS_PROFILING_ALLOW_REMOTE` 作为兼容 override 保留一个迁移周期。

## Risks / Trade-offs

- **[自采集反馈环导致数据爆炸]** → suppression scope + target denylist + 端到端“单事件至多一条”测试。
- **[日志洪峰增加延迟或内存]** → callback 只 `try_send`，per-signal bounded queue，drop metrics，不做无限重试。
- **[内部流绕过租户 quota 后占满存储]** → opt-in、7 天默认 retention、采样/队列/批大小上限；保留全局存储健康告警。
- **[CPU/heap profiling 有运行时开销]** → profiles 独立 opt-in、低 duty cycle、单 capture semaphore、heap 仅支持平台启用并在文档提示 jemalloc 持续采样成本。
- **[非 ingester 节点启动时无可用 ingester]** → 队列短暂缓冲 + bounded retry；健康指标显示不可达，不阻塞主服务。
- **[早期 bootstrap 尚未准备 `_sys`]** → subscriber 只保留 bounded startup buffer，system org 准备后再 drain；溢出明确计数。
- **[pprof 远程端点泄露代码/内存信息]** → loopback 默认、disabled 默认、独立 listener、remote 需显式开关和 Administrator auth。
- **[jemalloc 原生 dump 到 canonical pprof 的转换复杂]** → allocator adapter 隔离；先用 fixtures 锁定 stack/value 语义，不支持的平台返回 `501`。

## Migration Plan

1. 先引入配置、保留名校验、内部 origin 与 exporter health metrics，self-ingest 保持默认关闭。
2. 上线 logs/metrics/traces worker，验证 standalone 与 split-role 路由、递归和容量测试。
3. 补齐 pprof capture service 与 profiles stream override，再开放 scheduled/on-demand self profile persistence。
4. 文档提示现有用户：若已使用精确流名 `_molesignal`，升级前必须重命名；查询 API 不受影响，但后续写入/变更会被拒绝。
5. 回滚时关闭 `telemetry.self_collect.enabled` 和 `[profiling].enabled` 即停止新数据；已有 `_molesignal` 数据按 retention 保留并仍可查询。代码回滚不需要 schema migration。

## Open Questions

- CPU sampler 的具体 crate/feature 组合需在目标 Linux 架构上验证 unwind、frame-pointer 和 protobuf feature 的构建成本；行为契约不依赖最终选型。
- jemalloc dump 转 canonical pprof 的 adapter 是否采用纯 Rust 解析或隔离调用现有转换库，需以可移植性与测试 fixture 结果决定。
