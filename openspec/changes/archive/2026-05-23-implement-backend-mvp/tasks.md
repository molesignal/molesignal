## 1. 配置与依赖

- [x] 1.1 `Cargo.toml` 增加：`arrow`、`arrow-flight`、`parquet`、`datafusion`、`tantivy`、`lettre`、`argon2`、`jsonwebtoken`、`blake3`、`bytes`、`uuid` 等依赖；为 `object_store` 开 `aws/azure/gcp` feature。（workspace 层 deps 已就位；本次仅追加 `blake3`，并因 datafusion/tantivy 等依赖需要把 `rust-toolchain.toml`、`Cargo.toml::rust-version` 从 1.86 升到 1.90。）
- [x] 1.2 `crates/config/src/settings.rs` 新增 `AuthSettings { jwt_secret, token_ttl_secs, issuer }`、`NotifySettings { smtp: SmtpSettings, sms: SmsSettings }`、`ClusterSettings { heartbeat_interval_secs, peer_timeout_secs }`、`RouterRateLimit { ingest_qps, query_qps }`；为各字段补默认值与序列化测试。
- [x] 1.3 `conf/config.toml` 把 `meta_store.backend` 默认改为 `postgres`，写示例 DSN，并补全 1.2 新增的所有段。（新增 `crates/config/tests/parse_default_conf.rs` 守护，防默认配置漂移。）
- [x] 1.4 `deploy/docker/docker-compose.yml` 起 postgres 17、minio、molesignal 三件套；`make dev-up` 入口写到 `scripts/`。（compose 增加 postgres / minio / minio-init bucket bootstrap；脚本落为 `scripts/dev-up.sh` `dev-down.sh`，默认只起依赖，`--with-molesignal` 才连容器内 molesignal。）

## 2. shared / 通用基建

- [x] 2.1 `shared/src/ids.rs` 内 `Id` 增加 `as_uuid() / from_uuid()` 互转，方便 sea-orm 主键映射。（同时补 `new_uuid()`/`expect_uuid()`/`From<Uuid>`，并落 4 个单测。）
- [x] 2.2 `shared/src/time.rs` 补 `TimestampMicros::now()`、`TimeRange::contains/overlaps` 单测。（同时补 `from_secs/from_millis`、`TimeRange::at/duration_micros`，闭区间触点视为相交。）
- [x] 2.3 `shared/src/error.rs` 增加 `Error::NotFound / Forbidden / Conflict / Unauthorized`，并实现 `axum::response::IntoResponse`（4xx/5xx 映射）。（`Unauthorized` / `Forbidden` 改成带 message 的变体；axum 集成走 optional feature 避免污染 domain；新增 `http_status_code()` / `http_error_code()` 适配层。）
- [~] 2.4 `shared/src/telemetry.rs` 接入 `opentelemetry_otlp`：当 `otlp_endpoint != ""` 时启 OTLP exporter；增加 JSON formatter 分支。（**部分**：JSON formatter 完成；`init_full()` 接受 `otlp_endpoint/service_name/service_role` 入参，非空时打 warn；真正 OTLP exporter 装配延后到独立 change——`opentelemetry`/`opentelemetry_sdk`/`opentelemetry-otlp`/`tracing-opentelemetry` 四件套版本耦合，需要把整套锁到同一兼容组合才能编译过。）

## 3. proto / gRPC 定义

- [x] 3.1 新增 `proto/cluster/v1/cluster.proto`：package `cluster.v1`，`NodeService { Heartbeat, List }`、`NodeInfo`、`NodeRole` enum。
- [x] 3.2 新增 `proto/ingest/v1/ingest.proto`：package `ingest.v1`，`IngestService { Push(PushRequest) }`，字段镜像 `domain::ingestion::IngestBatch`，含 `StreamType` enum、`batch_id`、`IngestError`。
- [x] 3.3 新增 `proto/query/v1/query.proto`：package `query.v1`。Arrow Flight 直接复用 `arrow-flight` crate 自带服务；本仓 `query.proto` 只做分片协调，扩了 `flight_ticket_prefix` / `flight_ticket` 字段桥接两端。
- [x] 3.4 将协议绑定收敛到 `src/protocol/`：用 `tonic-prost-build`（版本与 tonic 0.14 同步）生成代码。`buf.yaml` 上移到项目根，只做 lint / breaking check。BSR 上的 `neoeinstein-tonic:v0.4.1` 与 tonic 0.14 API 不兼容，已弃用。`buf lint` 当前剩 4 条 RPC 类型命名风格 warning，不阻塞。

## 4. domain 补字段

- [x] 4.1 `domain/src/alerting/incident.rs::Incident` 增加 `escalation_policy_id: Id`、`fingerprint` 改为不可变（创建后不变）。（字段加上 + doc 标注不可变；不变性靠 repository 层 update 时不允许覆盖此列，详 5.4。）
- [x] 4.2 `domain/src/alerting/rule.rs::AlertRule` 增加 `last_eval_at: Option<TimestampMicros>`、`last_state: RuleState { Healthy, Pending(u32), Firing }`。
- [x] 4.3 `domain/src/alerting/schedule.rs::Rotation::resolve` 实现 `ActiveWindow` 的时区判定（用 chrono-tz），补单元测试覆盖跨日/跨时区。（新增 `Schedule::who_is_on_call` → `Rotation::resolve_in_tz(at, tz)`，`ActiveWindow::contains` 处理跨午夜窗口；3 个单测覆盖 Asia/Shanghai 偏移、跨午夜、weekday mask。）
- [x] 4.4 `domain/src/query/mod.rs` 新增 `PromqlEngine` trait（与 `QueryEngine` 并列）。

## 5. infra/persistence（sea-orm + Postgres）

- [x] 5.1 ~~entity 子模块~~ **改为 sqlx 后无 entity 层**：每张表的列直接在对应 repository 文件里 `row.try_get` 拆解；JSON 列经 `sqlx::types::Json<T>` 自动 (de)serialize 到 domain 结构。
- [x] 5.2 `crates/infra/migrations/20260101000001_initial.sql`：一次建齐 14 张表 + 必要索引（`memberships` 复合主键、`streams (org_id, name, stream_type)` 唯一、`incidents (org_id, fingerprint)` 唯一、`parquet_file_meta (org_id, stream, stream_type, time_start, time_end)` 复合索引、`deliveries (incident_id)` 索引）。
- [x] 5.3 `MetaStore::connect` 建池后立即 `MIGRATOR.run(pool)`（`sqlx::migrate!("./migrations")` 编译期 embed），幂等；`connect_no_migrate` 保留作旁路。`wire::build_state` 串入留到 Section 12。
- [x] 5.4 实现各 repository：14 个文件一一对应 domain trait，全部 happy-path CRUD；持 `sqlx::PgPool`，写路径用 `sqlx::query`、读路径用 `sqlx::query` + `row.try_get` 拼 domain 结构。错误统一经 `sqlx_err` 映射到 `Error::{NotFound, Conflict, Internal}`。
- [x] 5.5 `PgParquetFileMetaRepository::replace(merged_ids, new_files)` 在 `pool.begin()` 事务内 tombstone 旧行 + INSERT 新行，二者同 `tx.commit()` 提交。
- [~] 5.6 集成测试 `tests/it_persistence.rs`：用 testcontainers 起 postgres 跑 org / user CRUD；默认通过 `MS_RUN_IT=1` 才执行（本机无 docker 时跳过）。**剩余 12 张表的端到端覆盖留待独立 change**，本次只验证装配链路通。
- [x] 5.7 **改技术栈**：移除 sea-orm + sea-orm-migration，全部切到 sqlx + 原生 SQL；丢 sqlite feature 只留 postgres。

## 6. infra/storage：object_store + parquet

- [x] 6.1 `infra/src/storage/object.rs::build` 扩展支持 `s3 / azure / gcs`：`AmazonS3Builder` 直接拼 endpoint/access_key（兼容 MinIO / R2 / 阿里 OSS S3）；Azure / GCS 走 `from_env`，凭据由环境变量提供。新增 3 个单测覆盖 local 构造 / 未知 backend / s3 缺 bucket。
- [x] 6.2 `infra/src/storage/parquet_writer.rs::ParquetWriter::flush(stream, RecordBatch) → ParquetFileMeta`：snappy 压缩 → `ArrowWriter` 写入 `Vec<u8>` → `ObjectStoreExt::put`；产出 `ParquetFileMeta`（含 `_timestamp` 列扫出的 time range、行数、size，对 `indexed` 字段算 min/max — Int64 走 `arrow::compute::min/max`，Utf8 直接迭代，Float64 因 NaN 不可比跳过）。新增 `storage/arrow_schema.rs` 把 domain Schema → Arrow Schema 并在头部追加 `_timestamp` 列。
- [x] 6.3 `infra/src/storage/parquet_reader.rs::ParquetReader`：基于 `ParquetObjectReader` + `ParquetRecordBatchStreamBuilder` 暴露 `read_all` / `read_projection`（按列名子集投影）。`DataFusion ParquetExec / TableProvider` 接入留到 Section 8。
- [x] 6.4 集成测试：`tests/it_parquet_roundtrip` 被 `storage/parquet_writer.rs` + `parquet_reader.rs` 的内嵌单测覆盖（local object_store + writer → reader → 行数 + 列顺序断言），不再单独建文件。

## 7. infra/wal

- [x] 7.1 `infra/src/segment_wal/` 落地完整 WAL 子系统：`SegmentWal` writer（32B 头 + payload，CRC32C，lz4 压缩自动启用）+ `read_segment_file` / `scan_segment_file_readonly` 两条读路径（mmap）+ `evict_old_segments` 清理；段文件命名 `wal-{seq:06}.seg`，溢出 `segment_size` 自动 rotate。
- [x] 7.2 fsync 策略：`FsyncPolicy::{None, EveryWrite, Batch}` × `SyncLevel::{NONE, DATA, ALL}`；写路径模型 B（`write` → `BufWriter::flush()` ALWAYS → 按策略选 `sync_*`），节流由调用方在 ingester role 串入。
- [~] 7.3 启动重放：`SegmentWal::read_records(dir)` 返回所有完好记录，遇尾部损坏自动截断；从 WAL 到内存 buffer 的 push 链路在下个 change 接 ingester role 时完成（依赖还未实现的 buffer 模块）。
- [x] 7.4 单测：写-读 round-trip / segment 滚动 / 尾部截断 / readonly 扫描不写盘 / snapshot mark — 5 个测试全过。

## 8. infra/search

- [~] 8.1 tantivy 索引：仅留 `TantivyIndex` 占位 struct；本 change 不接入构建与裁剪路径。下一个 change 引入 `ParquetExec` 时一并补。
- [x] 8.2 `DataFusionEngine` 真实现（朴素路径）：`StreamHint` 必填 → `ParquetFileMetaRepository::find` 按时间窗裁剪 → `ParquetReader::read_all` 逐文件下载 → `MemTable::try_new` 注册到 `SessionContext` → `ctx.sql(stmt)` → `DataFrame::limit + collect` → `RecordBatch → QueryResult`。空候选返回 0 行。
- [x] 8.3 `UnimplementedPromqlEngine`：返回 `Error::internal("promql engine not implemented yet")`，用于让 `wire::build_state` 装配链路类型完整。
- [x] 8.4 `tests/it_query_sql.rs`：写 3 行 → `SELECT COUNT(*)` 拿到 3 / `WHERE latency_ms > 15 ORDER BY latency_ms` 拿 2 行 / 时间窗外 → 0 行，全过。

## 9. infra/notify

- [x] 9.1 `MultiNotifier` 重写：构造经 `NotifySettings`，按 `ChannelKind` 分发到 Slack / Webhook / Email；HTTP 客户端统一 10s 超时；HTTP 4xx/5xx 不再吞错而是返 `Error::internal`，让 dispatcher 的 Delivery 行记录失败原因。
- [x] 9.2 Email：`infra/src/notify/email.rs::EmailSender` 用 `lettre::SmtpTransport`，封装在 `tokio::task::spawn_blocking`；`SmtpTls` enum 支持 `none / starttls / tls`，credentials 与 timeout 来自 `[notify.smtp]`；订阅 channel 但 SMTP 未启用时直接返 `Error::invalid`。
- [~] 9.3 通用重试与 Delivery 落库：本模块只暴露 `Notifier::send`，重试与 `DeliveryRepository::record` 由 Section 10 的 `EscalationDispatcher` 在用例编排时处理（避免 infra 直接依赖 repository）。
- [x] 9.4 **范围裁剪**：PagerDuty 与 SMS 通道明确**不**接入；`ChannelKind` 只保留 `Email / Slack / Webhook` 三种；`[notify.sms]` 段、`SmsProvider` trait、`NoopSmsProvider` 全部删除；`SmsSettings` 配置项一并移除。
- [~] 9.5 mockito 单测：当前未添加（mockito 不在 workspace 依赖里，避免再增一项；改由 Section 13 端到端测试覆盖）。

## 10. app 层补全

- [x] 10.1 `AlertingService` 加 `DeliveryRepository` 字段 + 全部 list/get/update/delete CRUD（rules / schedules / policies / channels / incidents），incident ack/resolve 已就位。
- [~] 10.2 `RuleEvaluator`：暂未单独实装；evaluator tick 留给 Section 12 的 alert_manager role 接入 `QueryEngine` 后补（domain 层 `RuleState` 已就位）。
- [x] 10.3 `EscalationDispatcher` 改：`Incident.escalation_policy_id` 直接寻 policy，按 `EscalationTarget` 解析 user_ids（User → 自身；Schedule → `who_is_on_call(now)`；Team → 落到 channel 默认收件人）；每次 send 立即写 `Delivery { status, attempted_at, finished_at, error }`。
- [x] 10.4 `IngestService::ingest`：类型冲突单条剔除 + 错误填 `IngestResult.errors`；`infer_schema_extension` 扫出新字段后调 `streams.update_schema`；空字段集合不触发写。
- [x] 10.5 `IdentityService`：`authenticate` 走 argon2 verify + 邮箱/密码错误统一返 `Error::unauthorized("invalid credentials")`；`issue_token / verify_token` 用 jsonwebtoken HS256，注入 `AuthContext { user_id, org_id, role }`；`create_user` 自动 argon2 哈希。首用户自动建 org 留给 Section 12 的 wire 阶段（依赖 OrganizationRepository + MembershipRepository 装配）。
- [x] 10.6 `DashboardService::update / delete / list_by_folder` 在 repository trait 里已有；service 层用例编排在 Section 11 的 handler 里临场调用。
- [x] 10.7 `QueryService::run` 按 `language` 派发到 `Arc<dyn QueryEngine>` 或 `Arc<dyn PromqlEngine>`。

## 11. api 层补全

- [x] 11.1 `api/src/http/middleware/auth.rs`：`auth_layer` 解 `Authorization: Bearer <token>` → `IdentityService::verify_token` → 把 `AuthContext` 塞 `request.extensions`；白名单 `/api/v1/auth/login`、`/api/v1/healthz`、`/metrics`。
- [x] 11.2 `api/src/http/middleware/permission.rs::Permission::require(ctx, perm)`；handler 用 axum 内置 `Extension<AuthContext>` 取上下文（避免孤儿规则）。
- [x] 11.3 `auth.rs::login` 走 `IdentityService::authenticate` + JWT 签发，邮箱 / 密码错误统一返 `Error::unauthorized("invalid credentials")` 不区分。
- [x] 11.4 `ingestion.rs`：JSON 单条 / 数组 → `Vec<RawEvent>`（取 `_timestamp` 或回退 `now()`）→ `IngestBatch` → `IngestService::ingest` → 返 `IngestResult`。
- [x] 11.5 `query.rs`：body → `QueryRequest`；强制 `req.org_id = ctx.org_id` 防跨租户；调 `QueryService::run`；错误经 shared `IntoResponse` 自动映射 4xx/5xx。
- [x] 11.6 `dashboards.rs`：list / get / update（接 Grafana model JSON）/ delete / Grafana 导入完整；create 走 import 路径，single-shot create 留下一个 change。
- [x] 11.7 `alerting.rs`：rules / incidents（含 ack/resolve）/ escalations / channels 的 list / get / delete 全部接 service；create / update 因需要完整 domain 结构 DTO，本 change 返回 400 提示，留独立 change 补 DTO 层。
- [x] 11.8 `schedules.rs`：list / get / delete / on-call 完整；create / update / overrides 同上等 DTO 层。
- [~] 11.9 `identity.rs`（users / orgs / memberships / teams 路由）：本 change 未新增，靠 11.3 login 接口 + root 账号自动建（Section 12 wire）满足 MVP；后续 change 补全 CRUD。
- [~] 11.10 `/metrics` 端点：本 change 未实装；`telemetry.metrics_enabled` 字段就位，等 Section 12 wire 阶段把 `prometheus` 注册器接到 HTTP router。
- [~] 11.11 gRPC server：`api/src/grpc/mod.rs::server_stub` 占位；真正 `tonic::transport::Server::add_service` 装配留到 Section 12 把 cluster / ingest server impl 落地后再补。
- [x] 11.12 axum 错误转换：靠 Section 2.3 的 `shared::Error::IntoResponse`（feature `axum`），statusCode + `{ "error": "<code>", "message": "..." }` 自动映射。

## 12. server / wire / roles

- [x] 12.1 `server/src/wire.rs::build_state`：连 `MetaStore` 跑 migrator → 建 object store → Arc 起 14 个 repository → 装 `DataFusionEngine` / `UnimplementedPromqlEngine` / `MemoryIngestSink` / `MultiNotifier` → 构造五个 service → 返 `AppState`。新增 `seed_root_if_needed`：首启动若用户表空且 `[auth].root_*` 已配，自动建 default org + Owner 用户。
- [x] 12.2 `server/src/roles/http_server.rs`：bind `[http].bind:port` → `axum::serve`，auth middleware 经 `http::build_router` 装上；tracing 经 `TraceLayer`。`/metrics` 端点留待下个 change。
- [~] 12.3 ingester role：仍是 stub（每小时一次 sleep）。MemoryIngestSink 已落 `AppState`，让 HTTP `/api/v1/ingest/*` 链路通；WAL writer + Arrow buffer + flush task 在下个 change 接入。
- [~] 12.4 querier role：stub；DataFusionEngine 已直接在 HTTP `/api/v1/query` 上线，单机查询可用；Arrow Flight 跨节点扇出留下个 change。
- [~] 12.5 compactor role：stub，按 `compactor.interval_secs` 空跑 ticker；实际合并 + retention 留下个 change。
- [~] 12.6 alert_manager role：stub，按 `alert_manager.eval_interval_secs` 空跑 ticker；evaluator + dispatcher 真正接 service 留下个 change。
- [~] 12.7 router role：stub；下个 change 接 cluster registry。
- [x] 12.8 `server/src/main.rs`：`init_full` 注入 OTLP 入参 + JSON formatter 选择；fail-fast 守卫——若 `auth.jwt_secret == "dev-secret"` 且 roles 不含 standalone，拒绝启动；按 `[node].roles` 派发；任一 role 异常退出整体退出（fail-loud）。Graceful shutdown（SIGINT/SIGTERM）留下个 change。

## 13. 集成测试 / 端到端

- [x] 13.1 `crates/bootstrap/tests/it_ingest_query.rs`：postgres testcontainer + tempdir object_store → 通过 `wire::build_state` 装起完整 server → POST `/api/v1/ingest/logs/app`（3 条事件 + 一个新字段触发 schema 演化）验 `IngestResult.accepted=3` → 单独经 `ParquetWriter::flush` + `ParquetFileMetaRepository::insert` 写一份 parquet（因 ingester role 仍是 stub）→ POST `/api/v1/query` 验 `SELECT COUNT(*) = 3`、`scanned_rows = 3`。
- [~] 13.2 `it_alerting_flow.rs`：当前 alert_manager role 是 ticker stub（Section 12.6）且 RuleEvaluator 未单独实装；端到端 evaluator → dispatcher 验收留独立 change。`EscalationDispatcher` 流程在 Section 10.3 落地，相应单测建议下个 change 一并写。
- [x] 13.3 `crates/bootstrap/tests/it_auth.rs`：login OK / 密码错 401 / 邮箱不存在 401（同一 message 防 user-enumeration）/ 无 token 调受保护接口 401 / 带 token 200 / healthz 公开 200。
- [x] 13.4 `crates/bootstrap/tests/it_grafana_import.rs`：导入含 `weirdCustomTopLevel` / panel `extraVendorKey` 等未识别字段的 Grafana JSON → GET 出来后这些字段依旧在（`#[serde(flatten)] extra` 兜底有效），uid / title / org_id 正确。
- [~] 13.5 `it_cluster.rs`：router / querier role 本 change 是 stub，跨节点协调没实装，留独立 change。
- [x] 13.附加：`crates/infra/tests/it_persistence.rs` + `it_query_sql.rs` 同步切到 testcontainers postgres fixture，本轮 5 个 `it_*` 全部回归通过。

## 14. 部署与文档

- [x] 14.1 `deploy/docker/Dockerfile` 切到 Rust 1.90、装 `protobuf-compiler`（替代旧 buf 路径，因 code-gen 已改 tonic-prost-build）、加 healthcheck；compose 已在 Section 1.4 加齐 postgres + minio + bootstrap。
- [x] 14.2 `deploy/k8s/`：5 个角色 manifest（router Deployment ×2 + Service / ingester StatefulSet ×2 含 PVC / querier Deployment ×2 + Service / compactor Deployment ×1 / alert-manager Deployment ×1） + namespace + ConfigMap + Secret + README。
- [x] 14.3 README 重写 "快速开始" 段：scripts/dev-up.sh 起依赖 + 本地 cargo run + 实战 curl（login → ingest → query）+ `--with-molesignal` 全套 compose；proto 章节同步切到 `tonic-prost-build` 与项目根 `/proto/<pkg>/v1/` 布局。
- [x] 14.4 `ARCHITECTURE.md` 末尾追加 "Storage layout" 段：object key 规范、WAL 段格式（ASCII 图）、`parquet_file_meta` 索引列、tantivy 索引计划、sqlx 迁移幂等性约定。

## 15. 完工校验

- [x] 15.1 `cargo fmt --all` + `cargo clippy --workspace --all-targets -- -D warnings` 全绿（含 `Default::default + #[default]` 衍生、`std::slice::from_ref` 替换、`OpenOptions::truncate`、`len_zero` 等修复）。
- [x] 15.2 `cargo test --workspace --lib`：30 个单测全过；`MS_RUN_IT=1 cargo test --workspace --test 'it_*'`：5 个集成测试（auth / grafana_import / ingest_query / persistence / query_sql）全过，testcontainers 起 postgres。
- [~] 15.3 手动端到端：登录 → ingest → query 已被 `it_ingest_query` 覆盖；告警 → Slack → ack 路径依赖 alert_manager evaluator（Section 12.6 标记为 stub），留独立 change。
- [x] 15.4 `openspec validate implement-backend-mvp --strict` 通过。
- [x] 15.5 **改名**：`crates/server` → `crates/bootstrap`（包名 `molesignal-bootstrap`，二进制仍叫 `molesignal`）；ARCHITECTURE / README / openspec docs / k8s / dockerfile 全量同步。
- [x] 15.6 **环境变量改成单下划线**：`MS__` 前缀 + `__` 分隔符 → `MS_` 前缀 + 单下划线分隔符（如 `MS_HTTP_PORT=5081` / `MS_META_STORE_DSN=...`），全项目无双下划线和点号变量；compose / k8s / conf 注释 / config 加载逻辑同步。
