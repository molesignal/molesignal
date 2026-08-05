# Flight SQL Capability

## Purpose

对外 Arrow Flight SQL 查询协议：让 DBeaver / JDBC / ADBC 等数据库客户端直连查询遥测数据。独立 listener（默认 5083，区别于内部可信网络 gRPC 5082），每个 RPC 强制鉴权（API token 或账号密码换 JWT），复用 QueryService / DataFusion 引擎，org 隔离与 HTTP 查询一致。

## Requirements

### Requirement: Flight SQL listener 独立于内部 gRPC 端口

系统 SHALL 在 `flight_sql.enabled = true` 时启动一个独立的 tonic server（`flight_sql.bind:port`，默认 `0.0.0.0:5083`），仅挂载 Flight SQL 服务；内部 `QueryShard` ticket 协议与 `cluster.v1.NodeService` MUST NOT 在该端口可达。`flight_sql.enabled` 默认 MUST 为 false。

#### Scenario: 默认关闭

- **WHEN** 配置未显式设置 `flight_sql.enabled`
- **THEN** 进程不监听 Flight SQL 端口，现有 HTTP / gRPC(5082) 行为无任何变化

#### Scenario: 开启后独立监听

- **WHEN** `flight_sql.enabled = true` 且进程启动完成
- **THEN** Flight SQL 端口可被 Flight SQL 客户端连接，且对该端口发送内部 `QueryShard` ticket 的 `do_get` 被拒绝（不被识别为合法 Flight SQL ticket）

### Requirement: Handshake 鉴权与 per-RPC bearer 校验

系统 SHALL 在 Handshake 中按 password 形态分发两种凭据，复用既有认证体系：

1. password 以 `ms_` 开头 → API token 路径：经 `verify_api_token` 校验后把同一 token 作为 bearer 返回（username 忽略）。
2. 其余 → 账号密码路径：username 视作邮箱（可带 `@<org>` 选择子，见下），password 经 `IamService::authenticate` 校验，按所选 membership 经 `issue_token` 签发 JWT 作为 bearer 返回。SSO-only 用户无本地密码，此路径必然失败，MUST 引导使用 API token（文档说明）。

org 选择子：username 含 ≥2 个 `@` 时，最后一段 SHALL 解析为 org 选择子（匹配 org 的 name / slug / id）；恰含一个 `@` 时整体视作邮箱，沿用 login 的"第一个 membership"语义。选择子未命中任何 membership MUST 返回 `PERMISSION_DENIED`。

除 Handshake 外的每个 RPC（含全部元数据 RPC）MUST 携带 bearer 并按 HTTP 中间件相同的前缀规则重新校验（`ms_` → `verify_api_token`，其余 → `IamService::verify_token`），得到的 `IamContext` MUST 经数据库 IAM 能力解析并满足 `streams.read` 权限；缺失或非法 token MUST 返回 gRPC `UNAUTHENTICATED`，权限不足 MUST 返回 `PERMISSION_DENIED`。直接携带合法 bearer（`ms_` token 或未过期 JWT）而不经 Handshake 的客户端 SHALL 被同等接受。JWT 过期后 RPC 返回 `UNAUTHENTICATED`，客户端重新 Handshake 即恢复。

#### Scenario: API token 登录

- **WHEN** 客户端 Handshake 携带 basic auth，password 为有效未过期未吊销的 `ms_` token
- **THEN** Handshake 成功并返回该 token 作为 bearer，后续查询与元数据调用均可用

#### Scenario: 账号密码登录

- **WHEN** 客户端 Handshake 的 username 为已注册邮箱、password 为正确的账号密码
- **THEN** Handshake 成功，bearer 为 JWT，org 取第一个 membership（与 HTTP login 一致），后续查询与元数据调用均可用

#### Scenario: 多 org 用户指定 org

- **WHEN** 隶属多个 org 的用户以 username `alice@example.com@acme`（末段为 org name / slug / id）Handshake
- **THEN** bearer JWT 的 org 为 `acme` 对应的 membership；该用户不属于 `acme` 时返回 `PERMISSION_DENIED`

#### Scenario: 无效凭据拒绝

- **WHEN** Handshake 的 password 不是合法 `ms_` token，也不是正确的账号密码（含 SSO-only 用户）
- **THEN** 返回 `UNAUTHENTICATED`，连接上后续任何 RPC 均不可用

#### Scenario: 跳过 Handshake 直接带 bearer

- **WHEN** ADBC 客户端不调用 Handshake，直接在每个 RPC 上带 `authorization: Bearer <ms_ token 或合法 JWT>`
- **THEN** RPC 正常执行

#### Scenario: 权限不足

- **WHEN** token 对应身份没有 `StreamRead` 权限
- **THEN** 查询与元数据 RPC 返回 `PERMISSION_DENIED`

### Requirement: 只读 SQL 语句执行与 org 隔离

系统 SHALL 支持 `CommandStatementQuery`：`get_flight_info_statement` 返回内嵌 `{sql, org_id}` 的 ticket，`do_get` 解析 ticket 后 MUST 校验 bearer 所属 org 与 ticket org 一致，再经 `QueryService::run_tracked` 以 `QueryLanguage::Sql` 执行，org 范围 MUST 取自 `IamContext.org_id`。查询 MUST 进入 active query registry（可被 `/api/v1/query/running` 列出与 cancel）。非 SELECT 语句（DML/DDL）MUST 返回 `INVALID_ARGUMENT`。

#### Scenario: 基本查询

- **WHEN** 客户端执行 `SELECT * FROM logs.nginx LIMIT 10`
- **THEN** 返回该 org 下 `nginx` logs stream 的最多 10 行，编码为 Arrow `FlightData` 流

#### Scenario: ticket 跨 org 重放被拒

- **WHEN** 用 org A 的 ticket 配合 org B 的 bearer 调用 `do_get`
- **THEN** 返回 `PERMISSION_DENIED`

#### Scenario: 写语句拒绝

- **WHEN** 客户端执行 `INSERT INTO logs.nginx VALUES (...)` 或 `DROP TABLE ...`
- **THEN** 返回 `INVALID_ARGUMENT`，不触达查询引擎

#### Scenario: 查询可见可取消

- **WHEN** 一条 Flight SQL 查询执行中
- **THEN** `GET /api/v1/query/running` 能列出它，`POST /api/v1/query/{id}/cancel` 能取消它

### Requirement: stream 与时间窗推导

系统 SHALL 从 SQL 的第一个 base table 推导 `StreamHint`：schema 限定符映射 stream_type（`logs.` / `metrics.` / `traces.` / `extend.`），未限定时默认 `logs`。SQL 未提供分区裁剪信息时，`QueryRequest.time_range` SHALL 取 `now - flight_sql.default_lookback_hours` 至 `now`（默认 24h）。

#### Scenario: schema 限定的 stream_type

- **WHEN** 执行 `SELECT count(*) FROM metrics.cpu_usage`
- **THEN** 查询以 `StreamHint { name: "cpu_usage", stream_type: Metrics }` 执行

#### Scenario: 缺省回看窗口

- **WHEN** SQL 未包含可用于分区裁剪的时间信息且 `default_lookback_hours = 24`
- **THEN** 查询时间窗为最近 24 小时；SQL 中显式的 `_timestamp` 过滤条件仍由引擎执行

### Requirement: prepared statement 最小支持

系统 SHALL 实现无状态 prepared statement：`ActionCreatePreparedStatementRequest` 返回 handle（即 SQL 原文 bytes），`get_flight_info_prepared_statement` / `do_get` 路径与 ad-hoc 语句等价执行，`ActionClosePreparedStatementRequest` 为 no-op 并返回成功。参数绑定（DoPut）MUST 返回 `UNIMPLEMENTED`。

#### Scenario: JDBC executeQuery 路径

- **WHEN** JDBC 客户端以 prepared statement 方式执行无参 `SELECT`
- **THEN** create → get_flight_info → do_get → close 全链路成功，结果与 ad-hoc 路径一致

#### Scenario: 带参数拒绝

- **WHEN** 客户端对 prepared statement 执行 DoPut 参数绑定
- **THEN** 返回 `UNIMPLEMENTED`

### Requirement: 元数据浏览

系统 SHALL 实现 `CommandGetCatalogs`（单 catalog `molesignal`）、`CommandGetDbSchemas`（`logs` / `metrics` / `traces` / `extend`）、`CommandGetTables`（按 `IamContext.org_id` 列出 streams，schema 列为 stream_type，支持 filter pattern）、`CommandGetTableTypes`（`TABLE`）与 `CommandGetSqlInfo`（最小集：server name/version、read-only=true、identifier 引号规则）。

#### Scenario: DBeaver 浏览树

- **WHEN** DBeaver 连接后展开元数据树
- **THEN** 依次看到 catalog `molesignal`、四个 schema、以及当前 org 下各 stream 作为表

#### Scenario: org 隔离的表列表

- **WHEN** org A 的 token 调用 `CommandGetTables`
- **THEN** 结果仅含 org A 的 streams，不含其它 org

### Requirement: 结果编码为 Arrow

系统 SHALL 把 `QueryResult` 转换为单个 Arrow `RecordBatch` 返回：逐列推断类型（全 bool → Boolean；全整数 → Int64；数值混合 → Float64；其余含嵌套值序列化为 JSON 字符串 → Utf8；全 null → 可空 Utf8）；列名 `_timestamp` MUST 编码为 `Timestamp(Microsecond)`。空结果 SHALL 返回带 schema 的空流或零行 batch，MUST NOT 报错。

#### Scenario: 混合类型列退化

- **WHEN** 某列同时含数字与字符串值
- **THEN** 该列编码为 Utf8，数字按原文转字符串

#### Scenario: 时间戳列

- **WHEN** 结果包含 `_timestamp` 列（epoch micros）
- **THEN** 该列编码为 `Timestamp(Microsecond)`，DB 客户端按时间类型显示

#### Scenario: 空结果

- **WHEN** 查询命中 0 行
- **THEN** 客户端收到空结果集而非错误
