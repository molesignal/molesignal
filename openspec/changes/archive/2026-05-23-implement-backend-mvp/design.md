## Context

molesignal 是一个内部使用的 logs/metrics/traces 一站式可观测性平台，参考 OpenObserve 的存储/查询架构（Parquet + Object Store + Datafusion + Tantivy），后端采用 DDD 分层（domain / app / infra / api / server）。

当前状态：

- **domain**：trait、实体、枚举大体齐全（ingestion/query/alerting/dashboard/identity/storage/stream 共 7 个上下文）。
- **app**：service struct 已有，但调用链不完整（如 `DashboardService::import_grafana_json` 已可用，告警 `EscalationDispatcher` 大体齐但 incident 上还缺 `escalation_policy_id` 字段，规则评估器尚未存在）。
- **infra**：每个模块只有一段说明性 doc comment 或一个空 struct。`MultiNotifier` 只实现了 Slack/Webhook/PagerDuty 三家网络调用，Email/SMS、错误处理、Delivery 落库均未做；persistence 没有任何 entity；WAL、parquet writer/reader、tantivy 全是空文件。
- **api**：路由表完整但所有 handler 直接返回空 JSON。
- **server/wire**：`build_state` 显式返回 `Error::Internal`，进程根本起不来。
- **proto**：proto 目录已分 cluster/ingest/query，但没有 .proto 文件 commit；`build.rs` 调用 buf generate。

约束：

- workspace 已锁定 `edition = "2024"`、`rust-version = "1.86"`、`resolver = "2"`，依赖必须与现有 `Cargo.toml` 的版本号兼容。
- 依赖方向严格向下：`domain → 仅 shared`；`app → domain + shared`；`infra → domain + shared + config`；`api → app + domain + shared`；`server` 是唯一允许同时依赖以上所有的 crate。
- 用户已选 PostgreSQL 作为首批 meta store；object_store 必须支持 local/s3/azure/gcs 四种 backend。
- 不引入消息队列；集群路由先在 router 进程内存维护。

## Goals / Non-Goals

**Goals:**

- 一个能跑通 ingest → 存储 → SQL 查询 → 告警评估 → 通知 → ack/resolve 全链路的可执行 server。
- standalone / 多角色拆分两种部署模式都能起来。
- API 层走 JWT + Role 鉴权；查询和写入都按 org 隔离。
- infra 实现都符合 domain 端口，未来替换实现不动 app/api。
- 单元测试覆盖纯领域逻辑（schedule 解析、incident 状态机、policy 步进、schema 推断）；集成测试覆盖 ingest→查询、告警→通知两条主链路。

**Non-Goals:**

- 前端、UI 联调（前端 crate 已存在，由其他 change 推进）。
- 集群一致性 / 分布式事务：router 的节点表只在内存里，节点挂了靠心跳过期清理。
- 复杂权限模型：保留粗粒度 `Role` enum，不引入细粒度策略引擎。
- 查询缓存、采样降采样、租户配额、AI 异常检测、SSO/SAML/OIDC。
- 历史数据回填：retention 到期文件直接物理删除，不归档。

## Decisions

### 1. 持久层：sqlx 裸 SQL + 嵌入式迁移

- 不引 ORM：`sqlx::PgPool` + 原生 SQL，避免 sea-orm/diesel 的类型反射与额外抽象。只支持 Postgres。
- 表结构定义在 `crates/infra/migrations/*.sql`，启动期通过 `sqlx::migrate!("./migrations")` 编译期 embed + 运行期 `MIGRATOR.run(pool)` 应用未执行的迁移；表 `_sqlx_migrations` 由 sqlx 维护。
- 字段命名：表名复数 snake_case；主键 / 外键引用统一 `varchar(64)`（承载 KSUID 或 UUID 字符串）；时间戳 `BIGINT`（微秒，与 `TimestampMicros(i64)` 直接对应，避免反复转 chrono）；复杂结构 `JSONB`。
- Repository 层直接持 `PgPool`，每个方法 `sqlx::query` / `sqlx::query_as` + 手写 SQL + `row.try_get` → domain 结构；JSON 列经 `sqlx::types::Json<T>` 自动 (de)serialize。
- 错误统一经 `sqlx_err()` 映射：`RowNotFound` → `Error::NotFound`，SQLSTATE `23505` → `Error::Conflict`，其余 → `Error::Internal`。
- **替代方案**：sea-orm — 否决，对一个 14 表的 MVP 而言 ORM 的反射 + entity-derive 摩擦比直接 SQL 更高；diesel — 否决，async 体验弱、需要 schema.rs 同步；sqlx 编译期检查（`query!` 宏）— 否决，本 change 内不连数据库也能编译是硬要求。

### 2. object_store 多 backend：构造器在 `infra/storage/object.rs` 单点 match，feature 全开

- `Cargo.toml` 开 `object_store = { ..., features = ["aws","azure","gcp"] }`。
- 单个 `build(cfg)` 函数 match `backend`，分别用 `AmazonS3Builder` / `MicrosoftAzureBuilder` / `GoogleCloudStorageBuilder` / `LocalFileSystem` 构造。
- S3 兼容 endpoint 用 `with_endpoint(...)`（MinIO、Cloudflare R2、阿里云 OSS S3 接口都走这条）。
- **替代方案**：让 backend 字符串直接对应 `object_store::parse_url` — 否决，配置里散落 URL 比键值对难审计且不利于密钥分离。

### 3. WAL：基于 `segment_wal` 模块的分段日志，写路径强 flush + 可选 fsync 策略

- 模块位置：`crates/infra/src/segment_wal/`，对外暴露 `SegmentWal` writer、`scan_segment_file_readonly` reader、`FsyncPolicy` 配置。
- 目录布局：`{wal.dir}/{org}/{stream_type}/{stream}/`；段文件命名 `wal-{seq:06}.seg`，单段达到 `segment_size` 自动 rotate；超出 `max_segments` / `max_total_size_bytes` 由内置 `evict_old_segments` 清理。
- 记录格式：32 字节定长头 + payload，CRC32C（Castagnoli）覆盖头前 28B 与 payload，落在头内 `crc32c` 槽；`magic = 0xCA 0xFE`、`version = 2`、`flag` 低 7 位表示 entry type，bit7 表示 payload 经 lz4 压缩。
- entry types：`Normal`（ingest batch）/ `Config`（schema / 配置变更）/ `SnapshotMark`（parquet flush 边界，payload = 8B LE index）。
- 写路径模型 B：`write` → **`BufWriter::flush()` ALWAYS** → 按 `FsyncPolicy` 选择 `None` / `EveryWrite { sync_level }` / `Batch { max_pending, max_delay_ms, sync_level }`；`SyncLevel` 0=NONE, 1=DATA (`sync_data`), 2=ALL (`sync_all`)。
- 读路径两条：`read_segment_file` 用 mmap 解析，遇 CRC 错 / 字节不足截断到最后一条好记录；`scan_segment_file_readonly` 同样 mmap 但**永不写盘**，供工具 / 测试用。
- 重放：启动时 `SegmentWal::read_records(dir)` 顺序拿到所有完好记录，按 entry type 派发回 ingester buffer 与 stream schema cache。
- **替代方案**：sled / rocksdb — 否决，killing the simplicity；自实现 ISO-3309 CRC32 + bincode — 已弃用（旧版本），CRC32C + lz4 压缩 + mmap 在工程实测上更快更稳。

### 4. 内存 buffer 与 parquet flush：Arrow 表 per stream，按时间/大小双阈值触发

- buffer 用 `arrow_array::RecordBatch`，写入路径把 `RawEvent.fields` 按 stream schema 投影、缺字段填 null、不在 schema 的字段触发 schema 演化。
- 触发 flush 的条件 `OR`：`size_bytes >= ingester.buffer_max_mb*1MiB` 或 `oldest_batch_age >= flush_interval_secs`。
- flush 流水线：locked swap buffer → `ArrowWriter` 序列化到 `Vec<u8>` → `object_store.put_multipart` → 计算 min/max（仅 indexed 列）→ ParquetFileMeta INSERT → WAL truncate。
- **替代方案**：用 `parquet::arrow::AsyncArrowWriter` 直接流式 put — 留作后续优化，MVP 先在内存装好再 put 简化错误处理。

### 5. DataFusion 装配：SessionContext + ObjectStoreRegistry + 动态 TableProvider

- 每次查询新建一个 `SessionContext`（开销可接受，避免跨查询的 schema 缓存陷阱）；注册一个 `MoleSignalTableProvider`，其 `scan` 实现：
  1. 从 `ParquetFileMetaRepository::find` 拿候选文件列表 +TimeRange 裁剪 + min/max 二次裁剪。
  2. 构造 `FileScanConfig`（含 object_meta、partition values），用 `ParquetExec`。
  3. 如果谓词包含 `MATCH(field, term)`，扫 Tantivy 索引剔除无匹配的文件。
- `_timestamp` 是隐式列，所有 stream 都有。
- PromQL：`PromqlEngine` trait + 一个 `UnimplementedPromqlEngine`（返回 `Error::Internal("promql not yet implemented")`），让 API/wire 都能编译。
- **替代方案**：复用 OpenObserve 的 datafusion 适配 — 否决，他们绑了大量内部 hook；自己写执行器 — 否决，工作量远超 MVP。

### 6. Arrow Flight 分布式查询：querier 之间一层 fan-out，router 不参与计算

- querier 内部对 `ParquetFileMeta` 列表按 `len % querier_count` 哈希；hash 决定子查询去哪台。
- 子查询通过 `arrow-flight` `DoGet` 拉远端 RecordBatch 流，本地用 `MemoryStream` 拼回去再 union。
- **替代方案**：所有 querier 等价、router 做物理切分 — 否决，router 不应该懂 schema；不分片直接单机查 — 否决，本来就要支持多查询并发。

### 7. 告警评估循环：单 evaluator 任务 + per-rule next-eval 调度

- alert_manager 启动两个 tokio task：`rule_evaluator` 和 `escalation_dispatcher`，分别按 `eval_interval_secs` 和 `dispatch_interval_secs` 周期 tick。
- evaluator 每 tick 拉 `list_enabled()` → 对每条 rule 调 `QueryEngine::execute(period [now - period_secs, now])` → 取 result 的第 0 行第 0 列作为标量 → 比较 → 维护 rule 内存里的 `consecutive_hits` 计数器，达到 `for_periods` 才创建 incident。
- 状态机：rule 内存状态丢了重启从头数，可接受（首次重启会少 N 个周期的统计，但不会假告警）。
- fingerprint = `blake3(rule_id || labels_sorted_string)`，防同 rule 重复开多 incident。
- 在 `domain::alerting::incident::Incident` 上加 `escalation_policy_id: Id` 字段，dispatcher 不再走 "list policies 取第一条" 的占位逻辑。

### 8. EscalationDispatcher 精化：根据 target 类型解析人员

- `User → 直接 user_id`；`Schedule → schedule.who_is_on_call(now)`；`Team → team.member_ids`。
- 解析出来的 `user_ids` 喂给 `Notifier::send`（接口新增 `recipients: &[Id]` 参数；MultiNotifier 内部按 channel kind 决定怎么把 user 转成 email/手机号）。
- 每次 send 都创建一条 `Delivery`，无论成败；失败重试 3 次（指数退避 1s/4s/16s），仍失败则置 `Failed` 但不影响升级时序。

### 9. Notify：用 lettre 同步 SMTP 客户端跑在 spawn_blocking

- Email channel kind 在 `[notify.smtp]` 配段读 `host/port/username/password/tls`；构造一次 lettre `SmtpTransport` 复用。
- Slack/Webhook/PagerDuty 沿用 `reqwest::Client`，超时统一 10s。
- SMS：定义 trait `SmsProvider`，仓库内默认 NoOp，预留 Twilio/阿里云接入。

### 10. JWT + RBAC：自定义 axum middleware + extractor

- 中间件 `auth_layer` 解析 `Authorization`，注入 `AuthContext`；放行白名单 `["/api/v1/auth/login", "/api/v1/healthz", "/metrics"]`。
- 受保护 handler 通过自定义 `Permission(P)` extractor 做 `role.allows(P)` 校验，不通过直接 403。
- 密码用 `argon2 = "0.5"`；JWT 用 `jsonwebtoken = "9"`，HS256，secret 从 `[auth].jwt_secret` 读，默认开发环境用 `dev-secret`（必须警告且禁止生产使用）。

### 11. cluster 节点心跳：基于 gRPC + 内存表

- `proto/cluster/heartbeat.proto`：`service NodeService { rpc Heartbeat(NodeHeartbeat) returns (Empty); rpc List(Empty) returns (NodeList); }`。
- 节点启动时 spawn 一个 task 每 5s 调 `Heartbeat`；router 内存里维护 `HashMap<NodeId, (NodeInfo, last_seen)>`，每 5s 扫一次过期。
- standalone 模式短路：router 直接调 in-proc 服务，不走 gRPC 自环。

### 12. 配置默认值切换

- `meta_store.backend` 默认从 `sqlite` 改为 `postgres`，DSN 默认 `postgres://molesignal:molesignal@localhost:5432/molesignal`。
- 增加 `[auth]`、`[notify.smtp]`、`[notify.sms]`、`[cluster]`、`[router.rate_limit]` 子表。

## Risks / Trade-offs

- **PostgreSQL 强依赖会拉高本地起步成本** → 提供 docker-compose（molesignal + postgres 17 + minio），README 一行 `make dev-up`。
- **sqlite 退役但留 dev fallback** → 迁移用 sea-orm-migration 的 DB-agnostic SQL；CI 跑 postgres，sqlite 仅做编译/最小冒烟。
- **WAL fsync 与吞吐的折中** → 暴露 `wal.sync_interval_ms`；写入路径必须等 fsync 才 ack（持久性优先于吞吐）。
- **DataFusion SessionContext 每次查询新建有内存压力** → 评估后若 QPS 高再上 LRU；MVP 阶段优先正确性。
- **Tantivy 索引随 parquet 一起上传 + 下载会放大网络** → 给指定字段才建（看 `FieldDef.indexed`），不要给所有列开。
- **告警 evaluator 重启丢 `consecutive_hits`** → 接受，等首个 incident 即可通过 fingerprint 去重避免假告警。
- **router 节点表纯内存** → 多 router 进程不同步，需运行手册说明 "一个 cluster 一个 router 副本组共享后端"；下一阶段再上 etcd/Consul。
- **JWT secret 默认开发值** → 启动时若 `jwt_secret == "dev-secret"` 且 `node.roles` 不含 standalone-dev 模式，则 fail-fast。
- **object_store 全 backend feature 启用编译会变重** → 接受，运行期不影响；如 CI 慢可拆 feature gate。
- **OTLP exporter 装配延后** → `opentelemetry` / `opentelemetry_sdk` / `opentelemetry-otlp` / `tracing-opentelemetry` 四个 crate 版本高度耦合，0.31/0.32 系列的 `OpenTelemetryLayer` 与 stacked subscriber 不易满足 `SubscriberInitExt` bound；本 change 仅暴露 `init_full(otlp_endpoint, ...)` 入参并 log 提示，真正 exporter 装配拆到独立 change（届时锁定四件套到同一兼容组合）。`telemetry` spec 中相关 requirement 已视为已知缺口。

## Migration Plan

- 没有线上既存数据，无需数据迁移。
- 部署顺序：先起 postgres + minio → 跑 `migrator up` → 起 standalone 单进程冒烟 → 切到多角色拆分。
- 回滚：每次发布前打 git tag；schema 倒退靠 sea-orm-migration `down`。

## Open Questions

- PromQL 引擎选型（自研 vs 复用 `metricsql_engine`）放到下一个 change；本 change 只暴露 trait + 占位实现。
- 是否需要支持 OpenTelemetry collector 的 OTLP gRPC ingest？— 暂时只做 OpenObserve 兼容的 HTTP JSON 入口；OTLP gRPC 留作后续 capability。
- 多 router 节点表共享方案（etcd / consul / 直连 DB）— 留给下一个 change。
- 告警评估器是否需要支持多副本 leader 选举？— MVP 假设单副本 alert_manager，下一步加 lease 表锁。
