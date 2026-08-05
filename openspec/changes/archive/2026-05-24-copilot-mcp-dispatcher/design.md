## Context

MCP WebSocket 路由 + JSON-RPC envelope + `handle_request` 都已就位（spec M1）。`tools/list` 返 5 个 builtin tool 描述。但 `tools/call` 入口当前接 `NoopDispatcher`，相当于声明了能力但没实装。客户端（Claude Desktop / Cursor / 任意 MCP client）连进来跑工具会拿到 "not yet wired" 错误。

## Goals / Non-Goals

**Goals:**
- 5 个 builtin tool 真正打到 OSS subsystem 跑出结果。
- WebSocket session 全程保持 org_id / user_id 上下文，每个 tool 调用都做租户隔离 + role check。
- 每次 tool call 自动观测（写 `copilot_traces`）。

**Non-Goals:**
- 不实装第三方 MCP server 的 client（molesignal 是 server 端）。
- 不引入新 RBAC permission（沿用 caller 的 role，dispatcher 内调 `Permission::require` 与已有 HTTP handler 同标准）。
- 不实装 streaming tool 结果（MCP tool result 一次性返回；流式留 follow-up）。

## Decisions

### D1：dispatcher 持 `AppState` clone

实装 `RealToolDispatcher { state: AppState }`，构造时从 wire 阶段取 AppState clone（Arc 内部共享，cost 零）。原因：5 个 tool 涉及 `query` / `streams` / `incidents` 三个 dep，单独传太啰嗦；AppState 已是 Clone-able 设计。

### D2：tool 调用上下文 → 假 AuthContext

把 `McpAuthContext { user_id, org_id, role }` 转成 `app::identity::AuthContext`（已有结构）。这样可以直接复用 `Permission::require(...)`，与 HTTP handler 同代码路径。WebSocket upgrade 已校验 bearer token，dispatcher 不重复校验。

### D3：trace handler 提取共享函数

`crates/api/src/http/routes/web/trace.rs::rows_to_spans` 当前是 handler 内私有 fn。本 change 把它提到 `crates/app/src/web/trace_view.rs::rows_to_spans` 作为 pure function（不依赖 AppState），dispatcher 和 web handler 共用。架构上合规：app 已有 `web::aggregation` 兄弟模块。

### D4：copilot_traces 写入 best-effort

写 `copilot_traces` 用 `IngestService::ingest`，失败仅 warn 不影响 tool call 返回。原因：trace 观测属于辅助，不能让 ingest 失败拖死 chat 体验。

### D5：org_id / user_id 不可被 tool args override

dispatcher 完全忽略 `tools/call.arguments` 里的 `org_id` 字段（如果 LLM 偶尔生成了），全部走 `McpAuthContext.org_id`。理由：防止 prompt injection 跨租户读数据。

## Risks / Trade-offs

**[R1] WebSocket 长连接 + DB pool 耗尽**
→ Mitigation：dispatcher 内调 `state.query.run` 这种 await 的 future 不持锁；同一连接 tool call 串行（MCP 协议本来 request-response）。

**[R2] tool 跑慢拖延 WebSocket 心跳**
→ Mitigation：dispatcher 内调用上 `tokio::time::timeout(60s)`，超时返 `is_error: true`；客户端有显式 timeout 状态可显示。

**[R3] 5 个 tool 描述与实装漂移**
→ Mitigation：unit test 5 个 + 一个 `assert_tools_align`：把 builtin_tools() 的 name 列表与 dispatcher match arm 用 const 数组比对，新加 tool 必须两边同步。

**[R4] LLM args 不符合 schema**
→ Mitigation：每个 tool 实装内先 `serde_json::from_value` 拿强类型 args，反序列化失败返 `is_error: true` + 友好错误字串。
