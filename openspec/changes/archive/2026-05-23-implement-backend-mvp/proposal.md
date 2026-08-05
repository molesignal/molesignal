## Why

当前 molesignal 后端只是脚手架：domain 层的 trait 与实体已经齐全，但 infra 层（持久化、WAL、parquet、查询执行、通知）几乎全是模块说明 + `TODO`；`crates/bootstrap/src/wire.rs::build_state` 直接返回 `Error::Internal`，进程根本起不来；HTTP/gRPC 处理函数也全是 stub。缺一个完整的 MVP，就没法走 ingest → 存储 → 查询 → 告警 → 通知 这条主链路，更没法验证 DDD 分层假设是否合理。

参考 OpenObserve 的成熟实现（Parquet + Object Store + Datafusion + Tantivy），把骨架填实是这一阶段的明确目标。

## What Changes

- **Wire / Bootstrap**：`server/src/wire.rs::build_state` 真正装配 MetaStore + 全部 repository + 全部 service，并把 `AppState` 喂给 API；`main.rs` 启动时跑 sea-orm 迁移。
- **Persistence（sea-orm + PostgreSQL）**：补齐 entities / migration / pool / repositories，覆盖 stream、parquet_file_meta、dashboard、folder、organization、user、membership、team、alert_rule、incident、schedule、escalation_policy、notify_channel、delivery 共 14 张表的 trait 实现。
- **WAL**：按 `(org, stream, stream_type)` 分组的 segment 化 append-only 文件，含 fsync 节流、滚动、崩溃重放。
- **Storage**：parquet writer（Arrow RecordBatch → bytes → object_store::put + ParquetFileMeta 落库）、parquet reader（Object Store → Arrow），object_store 扩展到 **Local / S3 / Azure / GCS** 四种 backend。
- **Ingestion role**：HTTP `/api/v1/ingest/{logs,metrics,traces}/:stream` 与 gRPC `IngestService` 真正调用 `IngestService::ingest`；落 WAL → 内存 buffer → 周期 flush 到 parquet。
- **Search / Query**：`DataFusionEngine` 真正解析 SQL、查 `ParquetFileMetaRepository` 做分区裁剪、注册 `ParquetExec`、汇总 RecordBatch；PromQL 走单独路径（先桩接，能编译能解析即可）。`tantivy_index` 提供按 file_id 建/查倒排索引的最小路径。
- **Querier role**：监听 Arrow Flight，跨 querier 并行执行 ExecutionPlan，router 端做 fan-out + 合并。
- **Compactor role**：周期扫描小 parquet 文件，按 stream/day 合并并通过 `ParquetFileMetaRepository::replace` 原子替换；清理超过 retention 的数据。
- **Alert Manager role**：周期 tick 跑两件事 —— `RuleEvaluator` 对启用的规则执行查询并比较阈值 → 触发或解除 Incident；`EscalationDispatcher::tick` 按当前 step 通知 + 超时升级。
- **Notify**：补齐 Email（lettre SMTP）、SMS（占位但接口完备）、Slack/Webhook/PagerDuty 的错误处理与 `Delivery` 回执落库。
- **API routes**：所有 TODO 全部接入对应 service，含登录签发 JWT、query、ingest、dashboards CRUD + Grafana 导入、alerting 全部 CRUD、schedule on-call、incident ack/resolve。
- **Auth & Authz**：argon2 密码哈希、JWT 中间件、`Role::allows(Permission)` 在路由层强制。
- **Cluster gRPC**：节点心跳（`cluster.proto`）+ ingester 内部 ingest RPC（`ingest.proto`）的 server / client 实现，router 借此发现下游节点。
- **Config**：补 `auth`、`notify.smtp`、`cluster` 等子表的默认值；Postgres DSN 改作默认值之一。
- **Telemetry**：完善 `shared::telemetry`，把 OTLP exporter、`/metrics` 端点真正接上 settings。

非目标：前端、集群一致性算法（沿用 router 内存路由表）、复杂的 RBAC（保留粗粒度 Role）、查询缓存、租户级配额、采样/降采样、AI/ML 异常检测。

## Capabilities

### New Capabilities

- `ingestion`：日志/指标/Trace 的 HTTP + gRPC 写入、schema 校验/演化、WAL、内存 buffer、parquet flush。
- `storage`：parquet 文件读写、对象存储多 backend 支持、ParquetFileMeta 索引与分区裁剪、Tantivy 倒排索引、Compactor 合并/保留。
- `query`：SQL（DataFusion）执行、PromQL 入口、分布式 querier（Arrow Flight）扇出、结果汇聚。
- `alerting`：AlertRule 评估循环、Incident 生命周期（open/ack/resolved）、Schedule on-call 解析、EscalationPolicy 升级、Notify 多渠道派发与 Delivery 审计。
- `dashboard`：Folder/Dashboard CRUD、Grafana JSON 无损导入导出。
- `identity`：Organization/User/Team/Membership/Role/Permission CRUD、密码登录、JWT 签发与校验、RBAC 中间件。
- `cluster`：节点角色启动、router 反向代理 + 限流、节点心跳与下游发现、ingester/querier 之间的内部 gRPC/Arrow Flight。
- `telemetry`：结构化日志、OTLP traces 输出、`/metrics` Prometheus 端点、请求 ID 注入。

### Modified Capabilities

（尚无既有 spec，全部为新建。）

## Impact

- **Code**：`crates/{infra,app,api,server,config,shared}` 全部受影响；`domain` 仅做少量字段补全（如 `Incident.escalation_policy_id`、`AlertRule.last_state`）。
- **APIs**：HTTP `/api/v1/*` 全面落实；新增 gRPC `cluster.NodeService` / `ingest.IngestService` / Arrow Flight 端点。
- **依赖**：新增 sea-orm-migration（已在 lockfile）、tantivy、arrow-flight、lettre、argon2、jsonwebtoken、object_store 的 aws/azure/gcs feature。
- **数据库**：首批 PostgreSQL；sqlite 暂作为最小 dev fallback 但不保证全部测试通过。
- **配置**：`conf/config.toml` 增加 `[auth]`、`[notify.smtp]`、`[cluster]` 段，并把 `meta_store.backend` 默认从 sqlite 切到 `postgres`（开发环境 docker compose 起 Postgres）。
- **部署**：`deploy/docker` 增加 docker-compose（molesignal + postgres + minio 三件套），`deploy/k8s` 拆分 router / ingester / querier / compactor / alert_manager 五个 Deployment。
- **CI/测试**：集成测试需要 postgres 容器与本地 minio，加 `tests/it_*` 覆盖 ingest→查询→告警端到端。
