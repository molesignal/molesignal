## Why

Copilot chat SSE 路由已经接通（spec enterprise），但 `assistant` 消息当前是 placeholder（"LLM provider not wired in MVP"）。实际 token 流由真实 LLM 服务返回是 chat 能用的前提；缺这个适配层，整套 chat UI 就是个会写 user 消息进 DB 的壳。

## What Changes

- 新增 `ProviderAdapter` trait（已在 enterprise `copilot-chat` crate 定义）的 3 个具体实装：`OpenAiAdapter` / `AnthropicAdapter` / `OpenAiCompatibleAdapter`（兼容 Together / vLLM / Groq / Ollama 等同协议端点）。
- 用 `reqwest` 拉 streaming chat completion（SSE for OpenAI-style，`text/event-stream` JSON 增量 for Anthropic Messages API）。
- `copilot_chat` HTTP `post_message` handler 切到 `ChatLoop::run`：拉 session 历史 → 调 provider 增量 → 透传 SSE → tool call 时调 `ToolDispatcher` → 回灌 → 流到客户端。
- 每条 assistant 完成时按 `model_prices` 算 `cost_usd` 落 `chat_messages.cost_usd`（已有 catalog + compute 函数）。
- 失败处理：网络断 / token 余额耗尽 / 速率限制 → 落 `chat_messages` 一条 error role + 把 error 通过 SSE `event: error` 发给前端。

## Capabilities

### New Capabilities
<!-- 无 -->

### Modified Capabilities
- `copilot-chat`: 把"实际调 LLM"从 placeholder 升级为完整 ChatLoop。

## Impact

- **Enterprise crate**：`enterprise/crates/copilot-chat/src/` 新增 `providers/` 子模块，里面 3 个文件（`openai.rs` / `anthropic.rs` / `openai_compatible.rs`）+ 共享 SSE 解析 utility。
- **API crate**：`crates/api/src/http/routes/copilot_chat.rs::post_message` 重写为流式：用 `Body::from_stream(ChatLoop::run().stream())` 转发。
- **配置**：新增 `[copilot]` 段（`provider`、`api_key_env`、`base_url` 可选 / for openai_compatible），或允许 per-session 在 `chat_sessions.provider` + env-lookup 模式（推荐后者，避免全局耦合）。
- **依赖**：enterprise crate 加 `reqwest` + `tokio-stream` + `eventsource-stream`（解析 SSE chunks）。
- **测试**：3 个 provider 的 mock unit test（用 `wiremock`），1 个 ChatLoop tool-call 回灌的 integration test。
