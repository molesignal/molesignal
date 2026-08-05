## 1. Provider adapter 实装

- [x] 1.1 `enterprise/crates/copilot-chat/Cargo.toml` 加 `reqwest` (rustls + stream) + `tokio-stream` + `eventsource-stream` + `tokio-util` + `bytes`
- [x] 1.2 `enterprise/crates/copilot-chat/src/providers/sse.rs`：通用 `parse_sse(byte_stream) -> Stream<SseEvent>` + `data_lines_to_json` 工具函数
- [x] 1.3 `providers/openai.rs::OpenAiAdapter`：实装 `ProviderAdapter`，POST `{base_url}/chat/completions`，stream 解析 delta.content + tool_calls 增量拼装
- [x] 1.4 `providers/anthropic.rs::AnthropicAdapter`：POST `{base_url}/v1/messages` with `anthropic-version` header，stream 解析 content_block_delta + message_delta + usage
- [x] 1.5 `providers/openai_compatible.rs::OpenAiCompatibleAdapter`：直接复用 OpenAi 解析，仅可配 `base_url` + `api_key_env`
- [x] 1.6 unit test 3 个：每个 adapter 用 `wiremock` 起 mock server 验单 chunk 解析 + tool_call 解析

## 2. ChatLoop wire

- [x] 2.1 `enterprise/crates/copilot-chat/src/lib.rs::ChatLoop::run`：去掉 placeholder，真实 stream 实装（已有 trait 骨架）
- [x] 2.2 unit test：tool call 回灌（mock provider 返一次 tool_call → dispatcher mock 返结果 → 第二次调 provider 返文本）

## 3. API handler 重写

- [x] 3.1 `crates/api/src/http/routes/copilot_chat.rs::post_message`：删 placeholder 写法，改用 `ChatLoop::run` 返回的 stream 经 `Body::from_stream` 转发；保留 user message + cost 持久化
- [x] 3.2 配置：`crates/config/src/settings.rs` 加 `[copilot]` 段（`enabled`、`default_provider` 占位 / future use），但 API key 仍走 env
- [x] 3.3 env 解析 helper：根据 `session.provider` 取对应 `MS_COPILOT_<UPPER>_API_KEY` 与 `_BASE_URL`

## 4. Cost 持久化路径

- [x] 4.1 ChatLoop 完成时，从 provider 拿 `usage.prompt_tokens` / `usage.completion_tokens`
- [x] 4.2 调 `state.model_prices.get(provider, model)` → `compute_cost_usd` → 落 `chat_messages.cost_usd`
- [x] 4.3 unit test：known model 算出预期 cost；unknown model 落 NULL + warn 日志被记录

## 5. Error handling

- [x] 5.1 网络错 / 429 / 5xx → SSE `event: error` + 落一条 `role=assistant content="[error: …]"` row
- [x] 5.2 tool loop 超 `MAX_TOOL_LOOPS=8` → 同上但 reason = "tool loop budget exhausted"
- [x] 5.3 unit test：429 mock 触发 → SSE 末帧含 `event: error` + DB 落对应 error 消息

## 6. 编译矩阵 + 集成测试

- [x] 6.1 `cargo check -p molesignal-enterprise-copilot-chat` clean
- [x] 6.2 `cargo check -p molesignal-bootstrap --features enterprise` clean
- [x] 6.3 `cargo test -p molesignal-enterprise-copilot-chat --lib`：3 adapter + ChatLoop + cost 全绿
- [x] 6.4 `crates/bootstrap/tests/it_copilot_chat.rs`（require_docker + MS_RUN_IT=1）：起 wiremock 假 OpenAI → 真跑 SSE → 验 DB 落 row
