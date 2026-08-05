# Design: Arrow Flight SQL Endpoint

## Context

- 查询栈现状：HTTP `POST /api/v1/query`（`crates/api/src/http/routes/query.rs`）→ `QueryService::run_tracked`（`crates/app/src/query/mod.rs`，含 active query registry / cancel）→ `QueryEngine::execute`（DataFusion，分布式/联邦实现见 `crates/infra/src/query/`）。整条链路统一返回 `QueryResult { columns: Vec<String>, rows: Vec<Vec<serde_json::Value>>, scanned_rows, took_ms, federation }`，没有 RecordBatch 级别的对外 API。
- gRPC 现状：单个 tonic server 监听 `grpc.bind:port`（默认 5082，`crates/api/src/grpc/mod.rs`），挂 `IngestService`、`arrow_flight.FlightService`（内部 `QueryShard` ticket，集群内免鉴权）、`cluster.v1.NodeService`。该端口按"可信网络"假设部署，不暴露给终端用户。
- 鉴权现状：`verify_api_token`（`crates/api/src/http/middleware/auth.rs:74`）已是 pub，专门为跨集群 Flight do_get 复用而设计；`ms_<16>_<32>` token → argon2 verify → `AuthContext { org_id, user_id, role, ... }`。
- 依赖现状：workspace 已有 `arrow-flight = "58"`，其 `flight-sql` feature 提供 `arrow_flight::sql::server::FlightSqlService` trait（实现它即自动获得 `FlightService` blanket impl，可直接 `FlightServiceServer::new(...)` 挂载）。
- `QueryRequest` 必填 `time_range`（parquet_file_meta 分区裁剪靠它），`stream: Option<StreamHint>`（name + stream_type）；Flight SQL 协议本身没有时间窗概念，只有一条 SQL。

## Goals / Non-Goals

**Goals:**

- DBeaver（自带 Flight SQL 驱动）、Arrow Flight SQL JDBC、ADBC（Python/Go/C++）能直连 molesignal，浏览 stream 列表并执行只读 SQL。
- 每个 RPC 强制 API token 鉴权，org 隔离与 HTTP 路径完全一致；查询进入 active query registry，可被 `/api/v1/query/running` 看到、被 cancel。
- 不破坏现有内部 Flight shard 协议、不改 `QueryService` / `QueryEngine` 接口。

**Non-Goals:**

- 参数化 prepared statement（DoPut 参数绑定）、事务语义、DML/DDL。
- PromQL（Flight SQL 客户端只说 SQL；PromQL 仍走 HTTP）。
- 原生 RecordBatch 流式（v1 接受 `QueryResult` JSON→Arrow 的一次性转换；engine 级 streaming 是独立 follow-up）。
- 原生 TLS（v1 由 gRPC-aware ingress 终结 TLS；`tonic` TLS 留 follow-up）。
- 联邦查询 over Flight SQL（`federation_clusters` 固定为空，纯本地查询）。

## Decisions

### D1: 独立端口的专用 tonic server，而不是复用 5082

新起一个只挂 `FlightServiceServer<FlightSqlGrpc>` 的 tonic server（默认 5083）。

- 为什么不复用 5082：(a) tonic 一个 server 不能注册两个同路径的 `arrow.flight.protocol.FlightService`，复用就必须把内部 shard 协议和 Flight SQL 揉进同一个 service（可用 `do_get_fallback` 区分 ticket，但见 b）；(b) 5082 上的 shard do_get 和 NodeService 按"可信网络免鉴权"设计，而 Flight SQL 端口必须暴露给用户网络——同端口会迫使内部协议一起暴露，安全边界被打穿。独立端口让 ops 只暴露 5083，5082 继续留在内网。
- 代价：多一个 listener 和配置段。可接受。

### D2: 鉴权 = Handshake 双凭据分发 + 每 RPC 前缀分发校验，完全复用既有双 token 体系

- Handshake 按 password 形态分发（JDBC `user`/`password`、ADBC username/password 的标准路径）：
  - **API token 路径**：password 以 `ms_` 开头 → `verify_api_token` 校验后把同一 token 原样作为 bearer 返回，username 忽略 —— **无服务端 session 状态**。service account / 自动化首选。
  - **账号密码路径**：其余 → username 当邮箱、password 当账号密码，走 `IdentityService::authenticate`（与 `POST /auth/login` 同一函数），按所选 membership `issue_token` 签 JWT 作为 bearer 返回。人交互（DBeaver）首选，无需预先造 token。
- 多 org 选择子：邮箱恰含一个 `@`，故约定 username 含 ≥2 个 `@` 时末段为 org 选择子（匹配 org name / slug / id）：`alice@example.com@acme`。不带选择子时沿用 login 的"第一个 membership"语义。
- 后续每个 RPC（含元数据 RPC）从 bearer 按 **HTTP 中间件相同的前缀规则**重新校验：`ms_` → `verify_api_token`，其余 → `IdentityService::verify_token`（JWT，多 secret rotate window）→ `AuthContext`，再 `Permission::require(StreamRead)`。直接带 bearer 跳过 Handshake 的客户端（ADBC `authorization_header` 模式）天然支持。
- 已知边界（文档显著说明，spec scenario 覆盖）：
  - SSO-only 用户无本地密码 → 账号密码路径必然失败，只能用 API token（与 HTTP 行为一致）。
  - JWT 受 `token_ttl_secs` 过期 → RPC 返 `UNAUTHENTICATED`，DBeaver 用保存的凭据自动重连（重新 Handshake）即恢复；长脚本/自动化推荐不过期的 `ms_` token。
  - 密码会持久化在客户端连接配置里，泄露面大于可吊销、可设过期的 API token → 文档推荐"人用密码、自动化用 token"。

### D3: 查询执行复用 `QueryService::run_tracked`，结果 JSON→Arrow 单 batch

- ticket 设计：`get_flight_info_statement` 把 SQL（UTF-8 bytes）+ org_id 编进 `TicketStatementQuery.statement_handle`（prost 序列化的小结构 `FlightSqlTicket { sql, org_id }`），`do_get_statement` 解出后再次校验 bearer 的 org 与 ticket org 一致（防 ticket 跨 org 重放），然后构造 `QueryRequest { language: Sql, statement, time_range, stream, limit: None, federation_clusters: [] }` 调 `run_tracked(req, ctx.user_id)`。
- `QueryResult` → `RecordBatch`：按列扫描 JSON 值推断 Arrow 类型——全 bool → `Boolean`；全整数 → `Int64`；数值混合 → `Float64`；其余（含嵌套对象/数组，序列化为 JSON 字符串）→ `Utf8`；全 null 列 → `Utf8` nullable。单 batch 返回，规模与 HTTP 同界（受 query limit / matrix cap 约束）。
- 为什么不下钻 engine 拿原生 RecordBatch：`QueryEngine::execute` 的对外签名就是 `QueryResult`，原生 batch 要改 trait + 分布式/联邦两个实现 + registry 钩子，是大改造；v1 用转换换取零接口变更，类型保真度损失（如 timestamp 变 Int64 micros）记录为已知 trade-off，follow-up 解决。

### D4: 时间窗与 stream 提示从 SQL 推导，缺省回看窗口兜底

- stream：复用 `molesignal_infra::query::parser::extract_referenced_tables` 取第一个 base table 作 `StreamHint.name`。stream_type 通过 schema 限定符表达：`SELECT * FROM logs.nginx` → `Logs` + `nginx`；未限定 → 默认 `Logs`（与 HTTP `stream_query_get` 的默认一致）。元数据 RPC 暴露的表名也按 `<stream_type>.<stream>` 组织，引导客户端写限定名。
- time_range：v1 不做 WHERE 子句的 `_timestamp` 抽取（AST 抽取上下界是独立工作量），统一 `now - flight_sql.default_lookback_hours .. now`（默认 24h）。SQL 里写的 `_timestamp` 条件仍然生效（DataFusion 过滤），只是分区裁剪用缺省窗口。文档明确：查更久的数据需调大配置或等 follow-up（WHERE 抽取）。
- 为什么不要求客户端传时间窗：Flight SQL 协议没有自定义查询参数的标准通道，任何 header 约定都会破坏"标准客户端开箱即用"的目标。

### D5: prepared statement 最小实现（无状态），元数据 RPC 只读 streams repo

- JDBC 驱动 `executeQuery` 实际走 prepared statement 路径，必须实现：`CreatePreparedStatement` 返回 handle = SQL 原文 bytes（无服务端状态）；`get_flight_info_prepared_statement` / `do_get_prepared_statement` 与 statement 路径同逻辑；`ClosePreparedStatement` no-op；DoPut 绑参返回 `Unimplemented`（带参 SQL 直接报错，文档注明）。
- 元数据：`get_catalogs` 返回单 catalog `molesignal`；`get_db_schemas` 返回 `logs/metrics/traces/extend`；`get_tables` 按 `AuthContext.org_id` 查 streams repo，schema 列填 stream_type（支持 filter pattern）；`get_sql_info` 返回最小集（server name/version、read-only=true、identifier quoting）。`get_tables` 的表 schema（include_schema=true）返回 stream 定义里的推断 schema（`to_arrow`，含 `_timestamp`）——**不能给空 schema**：DBeaver 数据浏览页靠 JDBC `getColumns()` 建网格，空 schema = 0 列 = 打开表一片空白（冒烟实测）。

### D6: 配置与开关

`crates/config` 新增：

```toml
[flight_sql]
enabled = false          # 默认关闭，显式 opt-in
bind = "0.0.0.0"
port = 5083
default_lookback_hours = 24
max_message_size_mb = 32
```

`bootstrap` 在 `enabled = true` 时把新 server 加入现有 `tokio::try_join!`。不引入 license 闸门——与 HTTP SQL 查询同级的社区能力（联邦相关字段固定为空，不触碰 `federated_search` license 面）。

### 冒烟发现的客户端互操作约束（已实现为代码不变量）

1. **basic auth base64 必须 padding 宽容**：Rust/JDBC 客户端发规范 padding，ADBC（Go 驱动）发无 padding 的 std base64，严格解码会让所有 ADBC 用户握手失败（`decode_basic_b64`）。
2. **statement 的 `FlightInfo` 不携带 schema**："0 字段空 schema"会被 ADBC 当成预期 schema 与结果流严格比对（inconsistent schema 错误）；缺省时 ADBC 以数据流 schema 为准。
3. **prepared 的 `dataset_schema` 必须非空**：JDBC 驱动用"字段列表是否为空"分类 SELECT vs UPDATE，空 schema 会把 SELECT 引到 DoPut update 路径（结果全丢）。返回单字段占位 schema，ADBC 的一致性校验不比对此字段。

## Risks / Trade-offs

- [JSON→Arrow 类型损失：timestamp 退化为 Int64、数值列混入字符串时整列退化 Utf8] → 推断规则文档化；`_timestamp` 列名特判为 `Timestamp(Microsecond)` 提升客户端体验；follow-up 做 engine 级原生 batch。
- [缺省 24h 回看窗口让用户"查不到老数据"且不易察觉] → `get_sql_info` 与文档显著标注；结果元数据无标准通道，靠文档 + 后续 WHERE 抽取消除。
- [每 RPC argon2 校验在高频元数据调用下的开销] → 与 HTTP 一致的路径与缓存；DBeaver 元数据调用频率低，风险有限；必要时加 prefix→AuthContext 的短 TTL 内存缓存（follow-up）。
- [新端口被误暴露时的攻击面] → 默认 `enabled = false`；端口上无任何免鉴权 RPC（Handshake 失败即断）；ticket 内嵌 org 并与 bearer 二次比对，防跨 org 重放。
- [单 batch 返回大结果集的内存峰值] → 与 HTTP JSON 路径同界（同一 `QueryResult` 在内存里），不引入新的上界问题；超大结果引导用户走 HTTP async search-job。
- [arrow-flight `flight-sql` API 在大版本升级时仍可能变动] → 实现集中在单文件 `flight_sql_server.rs`，与业务逻辑（QueryService）解耦，升级面可控。

## Migration Plan

1. 合并后默认 `enabled = false`，零行为变化；现有 5082 内部协议不动。
2. 试点：在测试环境开启 5083，用 DBeaver + ADBC(Python) 各跑一轮连接/浏览/查询冒烟。
3. 生产开启时仅在 ingress 暴露 5083（gRPC + TLS 终结），5082 维持内网。
4. 回滚 = 配置关掉 `enabled`，无数据/schema 迁移。

## Open Questions

- ~~`get_table_schema` 是否值得用最近一个 parquet file 的 schema 近似返回，还是保持空 schema？~~ **已解决**：空 schema 会让 DBeaver 数据浏览页空白（见 D5），实现改为直接用 stream 定义里的推断 schema（比 parquet 近似更准且零额外 IO）。
- 默认回看 24h 还是 7d？先 24h（与查询成本保守对齐），按试点反馈调。
