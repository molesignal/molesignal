# 服务自身遥测回灌

MoleSignal 可以把进程自身的 metrics、traces 和 profiles 写回不可变系统组织 `_sys`。进程日志只输出到配置的终端或日志文件，不再回灌；升级前已经写入 `logs/_molesignal` 的历史数据不会被迁移或删除，仍按原有保留策略自然过期。启用后，每种自遥测信号使用各自的 `StreamType`，但流名统一为精确名称 `_molesignal`：

| 信号 | typed stream | 主要内容 |
|---|---|---|
| metrics | `metrics/_molesignal` | counter、gauge、histogram bucket/count/sum、summary quantile/count/sum |
| traces | `traces/_molesignal` | 已完成 span，字段契约与公共 OTLP trace 一致 |
| profiles | `profiles/_molesignal` | profile metadata；canonical pprof blob 仍归档到 object store |

所有事件共享稳定的进程资源字段：`service.name=molesignal`、`service.version`、`service.instance.id`、`service.role` 和 `node.id`。`service.instance.id` 在进程生命周期内不变，重启后重新生成。

本地兜底生成的根 `trace_id` 使用 UUIDv7 的 32 位小写十六进制无横线格式；`span_id` 使用操作系统随机源生成 8 字节，并编码为 16 位小写十六进制。有效的上游 Trace Context 保持不变。

## 启用

bootstrap 始终幂等创建或校验 `_sys`；首次启用某个信号时，会在该系统组织中创建对应的 typed stream 并应用 retention。目标组织不是配置项，不能改到普通租户。

```toml
[telemetry.self_collect]
enabled = true
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
```

配置中不接受旧的 `[telemetry.self_ingest]`、`telemetry.trace.self_ingest_enabled`，也不接受 `org_slug`、`logs_enabled`、`logs_retention_days`、`traces_enabled` 或 `profiles_enabled`；出现这些字段会被当作未知字段拒绝。运行时始终通过常量 `SYSTEM_ORG_SLUG = "_sys"` 解析系统组织。开启 `telemetry.self_collect.enabled` 后固定启动 profiles；metrics 仍可通过 `metrics_enabled` 单独关闭。Trace 只有在该总开关开启且 `telemetry.trace.enabled` 的有效策略允许捕获时才写入 `_sys/traces/_molesignal`；关闭 self telemetry 不影响独立配置的外部 OTLP Trace 导出。

## 权限和写保护

`_molesignal` 只保留这个精确名称；`_custom` 等其他下划线前缀不受影响。

- 公共 HTTP、OTLP、Prometheus、兼容协议、connector、profile 和 gRPC 写入不能选择 `_molesignal`。
- 可信 self-telemetry 写入入口同时校验 bootstrap 注入的 `_sys` 组织 ID 与精确流名 `_molesignal`；即使内部调用方传错组织，也不会落入普通租户。
- Stream CRUD 不能创建或删除 `_molesignal`，pipeline 也不能把它设为目标；现有系统流允许修改安全的字段索引、提取和遮掩设置。
- 只有切换到 `_sys` 的 `system_scope` 且具有系统遥测读取权限的平台管理员能列出和查询这些流；普通租户不可见。
- split-role 内部 RPC 需要各节点设置相同的 `MS_SELF_TELEMETRY_CLUSTER_TOKEN`。该值只从环境读取，不写入日志或遥测字段。

## standalone 与 split-role

standalone/ingester 节点直接调用可信内部 ingestion，继续经过 schema evolution、masking、WAL 和 drain gate，但跳过用户 pipeline。

router、querier、compactor 和 alert-manager 节点从 cluster registry 选择 ingester，通过带内部 origin 和 bearer 的 gRPC 发送。批次中的 resource identity 来自生产节点，不会被接收 ingester 改写。远程发送最多尝试 3 次，并受 10 秒队列年龄上限约束。

## 背压、递归和关闭

traces 使用独立有界队列。`tracing` callback 只执行 `try_send`，队列满时直接丢弃，不阻塞业务请求。metrics 和 profiles 是定时 producer。

内部 worker、远程路由和 profile 归档运行在 suppression scope 中；self-telemetry 模块自身的 tracing target 也被过滤，因此写入产生的日志不会再次回灌。正常关闭时先停止 producer 并在 `flush_timeout_secs` 内冲刷队列，再进入节点 drain；超时只丢弃尚未落入 WAL 的记录，不阻塞进程退出。

可通过 `/metrics` 观察 exporter 本身：

- `self_telemetry_accepted_total{signal}`
- `self_telemetry_dropped_total{signal,reason}`
- `self_telemetry_batches_total{signal,outcome}`
- `self_telemetry_retries_total{signal,reason}`
- `self_telemetry_queue_depth{signal}` / `self_telemetry_queue_capacity{signal}`
- `self_telemetry_last_success_unixtime{signal}`
- `self_telemetry_profile_available{kind}`
- `db_pool_connections{pool="meta",state="total|idle|checked_out|min|max"}`
- `db_pool_acquire_duration_seconds{pool="meta"}`

这些 labels 来自固定枚举，避免 exporter 健康指标产生高基数。

元数据库连接池默认通过 `store.meta.min_connections = 2` 在启动阶段预热连接。`db_pool_acquire_duration_seconds` 直接记录 SQLx 从连接池取得连接的等待时间，不包含 SQL 执行时间。集群节点发现结果在每个进程内缓存 2 秒，不同角色的并发查询共享一次刷新，避免 router 或 self-telemetry 批次逐次查询 `cluster_nodes`。

## 存储成本与查询示例

默认保留 7 天。指标 family 数量和 profile duty cycle 会直接影响存储；建议先在单节点或较短 retention 下观察 `accepted`、`dropped` 与实际 object-store 增长。profile blob 使用现有 `profiles/<org>/<service>/<type>/<date>/<id>.pprof.zst` 布局。

SQL 查询时同时传递 stream hint，区分三个同名 typed streams。例如：

```sql
-- metrics
SELECT metric_name, metric_kind, value, "service.role", "node.id"
FROM "_molesignal"
WHERE metric_name = 'self_telemetry_dropped_total';

-- traces
SELECT trace_id, span_id, parent_span_id, name, duration_ns, status_code
FROM "_molesignal"
ORDER BY duration_ns DESC
LIMIT 100;

-- profiles metadata
SELECT service, profile_type, duration_nanos, sample_count, object_key
FROM "_molesignal"
ORDER BY "_timestamp" DESC;
```

对应的 query request 中分别设置 `stream.stream_type` 为 `metrics`、`traces` 或 `profiles`，并设置 `stream.name` 为 `_molesignal`。
