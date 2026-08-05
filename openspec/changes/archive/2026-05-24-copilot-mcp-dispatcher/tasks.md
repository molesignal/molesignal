## 1. 共享 spans 解析提取

- [x] 1.1 把 `crates/api/src/http/routes/web/trace.rs::rows_to_spans` 提到 `crates/app/src/web/trace_view.rs`
- [x] 1.2 web handler 改为调 `molesignal_app::web::trace_view::rows_to_spans`
- [x] 1.3 unit test 跟着搬过去（行映射 + 截断阈值）

## 2. RealToolDispatcher 实装

- [x] 2.1 `crates/api/src/http/routes/copilot_mcp_dispatcher.rs`（cfg=enterprise）：`RealToolDispatcher { state: AppState }`
- [x] 2.2 `impl ToolDispatcher`：match `call.name` → 5 个 arm + 默认 `is_error: true`
- [x] 2.3 每个 arm 内 `let args = serde_json::from_value(call.arguments).map_err(...)?`
- [x] 2.4 `query_logs` / `query_metrics` arm：调 `state.query.run(req)` → 把 `QueryResult` 转 `ToolContent::Json { json: { columns, rows, scanned_rows, took_ms } }`
- [x] 2.5 `list_streams` arm：`state.streams.list(org)` 过滤 `args.stream_type`
- [x] 2.6 `get_trace` arm：构 `QueryRequest`（traces stream + `WHERE trace_id = '..'`）→ `rows_to_spans` → 转 JSON
- [x] 2.7 `list_recent_alerts` arm：`state.alerting.incidents.list_active(org)` 限 `args.limit`

## 3. WebSocket upgrade 接 real dispatcher

- [x] 3.1 `routes/copilot_mcp.rs::upgrade`：构造 `RealToolDispatcher::new(state.clone())` 注入 `handle_socket`
- [x] 3.2 `handle_socket` 签名加 `dispatcher: Arc<dyn ToolDispatcher>` 入参，删 Noop

## 4. Trace 观测

- [x] 4.1 `RealToolDispatcher::dispatch` 内 `let t0 = Instant::now();` 包围实际 tool 调用
- [x] 4.2 完成后 `tokio::spawn` 写一条 `copilot_traces` event；失败仅 `tracing::warn`
- [x] 4.3 unit test：build_trace_event 验证 tool_name + duration_ms 字段

## 5. Org isolation

- [x] 5.1 unit test：org=A session 调 `list_streams` → 仅返 org A 的 streams
- [x] 5.2 unit test：args 里硬塞 `org_id: "B"` → dispatcher 忽略，仍以 McpAuthContext.org_id 为准
- [x] 5.3 unit test：未知 tool name → `is_error: true` + "unknown tool: <name>"

## 6. 编译矩阵 + 集成测试

- [x] 6.1 `cargo check --workspace` clean（OSS 不引 dispatcher）
- [x] 6.2 `cargo check -p molesignal-bootstrap --features enterprise` clean
- [x] 6.3 `cargo test -p molesignal-api --lib copilot_mcp_dispatcher::` 全绿
- [x] 6.4 `crates/bootstrap/tests/it_copilot_mcp.rs`：起 axum + 客户端 tokio-tungstenite 连 WS，跑 `initialize` → `tools/list` → `tools/call: list_streams` 全链
