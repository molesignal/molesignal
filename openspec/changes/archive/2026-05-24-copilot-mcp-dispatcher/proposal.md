## Why

MCP WebSocket 路由已上线（spec M1），客户端可连 `/api/v1/copilot/mcp` 跑 `initialize` / `tools/list`。但 `tools/call` 当前接的是 `NoopDispatcher`，对每个 tool 调用都返"dispatcher not yet wired"。要让 Claude Desktop / Cursor / Continue 这类 MCP 客户端真能调到 molesignal 的 5 个 builtin tools（`query_logs` / `query_metrics` / `list_streams` / `get_trace` / `list_recent_alerts`），必须接真实 dispatcher。

## What Changes

- 新增 `RealToolDispatcher`（位于 api crate 或 bootstrap workers），impl enterprise `ToolDispatcher`。
- 5 个 builtin tool 的 dispatch 路径：
  - `query_logs` / `query_metrics` → 经 `QueryService::run` 跑 SQL/PromQL，结果转 MCP `ToolResult.Json`
  - `list_streams` → `StreamRepository::list(org)` 过滤 + 序列化
  - `get_trace` → 走现有 `/api/v1/web/trace/{id}` handler 的 span query 路径（复用 `trace.rs` 的 `rows_to_spans` 函数）
  - `list_recent_alerts` → `IncidentRepository::list_active(org)` 序列化
- WebSocket upgrade handler 注入 dispatcher 由 wire 阶段构造的 `Arc<RealToolDispatcher>` 替换 noop。
- 每次 tool call 写一条 `copilot_traces` event（type=mcp_tool_call，含 tool_name / duration_ms / status），与 chat trace 形式一致，方便统一观测。

## Capabilities

### New Capabilities
<!-- 无 -->

### Modified Capabilities
- `copilot-mcp`: 替换 NoopDispatcher，把 5 个 builtin tool 接到 OSS 主仓的 QueryService / StreamRepository / IncidentRepository。

## Impact

- **API crate**：新增 `crates/api/src/http/routes/copilot_mcp_dispatcher.rs`（cfg=enterprise），实装 `ToolDispatcher`；从 `AppState` 拿 `query` / `streams` / `incidents` 三个 dep。
- **WebSocket handler 改造**：`copilot_mcp.rs::handle_socket` 加 `dispatcher: Arc<dyn ToolDispatcher>` 入参，不再 hardcode Noop。
- **trace 复用**：把现有 `routes/web/trace.rs::rows_to_spans` 提到 `app::web::trace_view::rows_to_spans` 公共位置（架构允许，app 已有 `web` 子模块）。
- **不动 schema**：`copilot_traces` 是流不是表，写入走 `IngestService::ingest`。
- **测试**：5 个 tool 各一个 unit test（mock QueryService / StreamRepository），1 个 integration test 全链路。
