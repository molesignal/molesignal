# Tasks: Arrow Flight SQL Endpoint

## 1. 依赖与配置

- [x] 1.1 workspace `Cargo.toml`：`arrow-flight = { version = "58", features = ["flight-sql"] }`，确认 `cargo check` 通过
- [x] 1.2 `crates/config`：新增 `FlightSqlSettings { enabled(false), bind("0.0.0.0"), port(5083), default_lookback_hours(24), max_message_size_mb(32) }`，挂到根 Settings，补默认值测试
- [x] 1.3 `src/protocol`（或 api 内部模块）：定义 ticket 用的 `FlightSqlTicket { sql, org_id }` prost 消息

## 2. 鉴权与请求构造

- [x] 2.1 `crates/api/src/grpc/flight_sql_server.rs`：骨架 `FlightSqlGrpc`（持有 `QueryService`、`ApiTokenRepository`、streams repo、settings），实现 `FlightSqlService` 并默认全部 RPC 返 `Unimplemented`
- [x] 2.2 Handshake：解析 basic auth，password 走 `verify_api_token`，成功后以同一 token 回 bearer（`HandshakeResponse.payload` + `authorization` header）
- [x] 2.3 per-RPC 鉴权 helper：从 request metadata 取 bearer → `verify_api_token` → `Permission::require(StreamRead)`，错误映射为 `UNAUTHENTICATED` / `PERMISSION_DENIED`
- [x] 2.4 SQL → `QueryRequest` 构造器：`extract_referenced_tables` 推导 StreamHint（schema 限定符 → stream_type，未限定默认 logs）、`default_lookback_hours` 时间窗、拒绝非 SELECT（sqlparser AST 判断，DML/DDL 返 `INVALID_ARGUMENT`），单测覆盖四种 stream_type 限定与拒绝路径

## 3. 语句执行与结果编码

- [x] 3.1 `get_flight_info_statement`：校验 SQL 合法性，签发内嵌 `{sql, org_id}` 的 ticket
- [x] 3.2 `do_get_statement`：解 ticket → org 一致性校验 → `run_tracked` → 编码 FlightData 流返回
- [x] 3.3 `QueryResult` → `RecordBatch` 转换器（独立模块 + 单测）：类型推断规则、`_timestamp` → Timestamp(Microsecond)、混合列退化 Utf8、嵌套值转 JSON 字符串、空结果零行 batch
- [x] 3.4 prepared statement：create（handle = SQL bytes）/ get_flight_info / do_get / close(no-op)；DoPut 绑参返 `UNIMPLEMENTED`

## 4. 元数据 RPC

- [x] 4.1 `get_catalogs` / `get_db_schemas` / `get_table_types` / `get_sql_info`（read-only=true、server name/version、quoting 规则）
- [x] 4.2 `get_tables`：按 `AuthContext.org_id` 查 streams repo，schema 列 = stream_type，支持 filter pattern；org 隔离单测

## 5. 装配与启动

- [x] 5.1 `crates/api/src/grpc/mod.rs`：新增 `serve_flight_sql(state, settings)`，独立 tonic server 仅挂 `FlightServiceServer<FlightSqlGrpc>`
- [x] 5.2 `crates/bootstrap`：`flight_sql.enabled` 时把新 server 并入 `tokio::try_join!`；启动日志带监听地址

## 6. 测试与文档

- [x] 6.1 集成测试：`arrow_flight::sql::client::FlightSqlServiceClient` 走 Handshake → get_tables → 执行 SELECT → 校验行数与类型；无效 token / 跨 org ticket / DML 拒绝三个反例
- [x] 6.2 集成测试：Flight SQL 查询出现在 query registry（list_for 可见）且 cancel 生效
- [x] 6.3 本地冒烟：DBeaver（Flight SQL 驱动）与 ADBC Python 各连一次，浏览树 + 查询截图记录到 PR
- [x] 6.4 文档：docs 站新增 "Connect with a database client" 页（连接串、token 配置、`<stream_type>.<stream>` 命名、24h 缺省窗口与限制说明）

## 7. 账号密码登录接入（复用既有认证体系）

- [x] 7.1 `FlightSqlGrpc` 注入 `IdentityService`；Handshake 按 password 形态分发：`ms_` → API token 路径；其余 → `IdentityService::authenticate` + membership 选择 + `issue_token` 签 JWT 回 bearer
- [x] 7.2 username org 选择子：`<email>@<org>`（≥2 个 `@` 时末段匹配 org name/slug/id；未命中 → `PERMISSION_DENIED`），单测覆盖解析
- [x] 7.3 per-RPC 鉴权改前缀分发（与 HTTP 中间件一致）：`ms_` → `verify_api_token`，其余 → `verify_token`（JWT）
- [x] 7.4 集成测试：账号密码 Handshake → 查询；`email@org` 选 org；错密码 / 错 org 拒绝
- [x] 7.5 文档：鉴权章节改写 —— 两种凭据、org 选择子、"人用密码/自动化用 token" 推荐、SSO-only 用户限制、JWT 过期重连行为
