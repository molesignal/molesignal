# Add Arrow Flight SQL Endpoint

## Why

molesignal 的遥测数据目前只能通过 HTTP JSON API（`POST /api/v1/query`）用 SQL 查询，标准数据库客户端（DBeaver、DataGrip、JDBC/ADBC 应用、BI 工具）无法直接连接。查询引擎本身已经是完整的 SQL（sqlparser + DataFusion），缺的只是一个标准传输层。`arrow-flight` 已是 workspace 依赖且自带稳定的 `flight-sql` feature，补一个 Flight SQL endpoint 即可让所有 Flight SQL 生态客户端直连，且结果走 Arrow 列式编码，比 JSON 序列化高效。

## What Changes

- 新增独立的 Flight SQL gRPC listener（默认端口 5083，独立于内部 gRPC 5082），实现 `arrow_flight::sql::server::FlightSqlService`：
  - Handshake 鉴权双路径：password = `ms_` API token（自动化），或 username/password = 邮箱 + 账号密码（人交互，复用 `IdentityService::authenticate` 签 JWT；username 末段可带 `@<org>` 选择 org）；后续每个 RPC 按 HTTP 中间件相同前缀规则校验 bearer（`ms_` → API token，其余 → JWT）→ `AuthContext`，强制 org 隔离与 `StreamRead` 权限。
  - Ad-hoc 查询（`CommandStatementQuery`）与最小化 prepared statement（无参数绑定），覆盖 JDBC/ADBC/DBeaver 的实际调用路径。
  - 元数据 RPC（catalogs / schemas / tables / table types / sql_info），让 DBeaver 能浏览 stream 列表（schema 即 stream_type：`logs` / `metrics` / `traces` / `extend`）。
  - 查询执行复用现有 `QueryService::run_tracked`（含 active query registry / cancel / 配额），`QueryResult` JSON rows 转换为 Arrow `RecordBatch` 返回。
- 新增 `flight_sql` 配置段（`enabled`（默认 false）、`bind`、`port`、`default_lookback_hours`）；SQL 未携带 `_timestamp` 条件时使用默认回看窗口。
- 内部 shard 执行用的 `FlightService`（`QueryShard` ticket）保持原样，不对外暴露在新端口上。

## Capabilities

### New Capabilities
- `flight-sql`: Flight SQL 对外查询协议——监听、鉴权与 org 隔离、语句执行、元数据浏览、结果编码、配置开关。

### Modified Capabilities
<!-- 无：query / api-tokens 的既有需求不变，本变更只新增传输层并复用现有行为。 -->

## Impact

- **新代码**：`crates/api/src/grpc/flight_sql_server.rs`（FlightSqlService 实现）、`crates/api/src/grpc/mod.rs`（新增独立 tonic server 装配）、`crates/bootstrap`（条件启动新 listener）、`crates/config`（`FlightSqlSettings`）。
- **依赖**：`arrow-flight` 增开 `flight-sql` feature（版本不变，无新 crate）。
- **复用不改**：`QueryService`、`verify_api_token`、`Permission`、streams repo、query registry。
- **安全面**：新增一个需要暴露给用户网络的端口；默认关闭，每个 RPC 强制鉴权；内部免鉴权的 shard 协议不在该端口提供。TLS v1 由 gRPC-aware ingress 终结，原生 TLS 留 follow-up。
- **不在范围**：参数化 prepared statement（DoPut 绑参）、事务、写入路径（DML/DDL 一律拒绝）、PromQL over Flight SQL、原生 RecordBatch 流式管线（v1 单 batch，与 HTTP 行为一致）。
