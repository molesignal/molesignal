## Context

Chat session + message 持久化 + cost catalog + ChatLoop trait + ToolDispatcher trait 都已就位（spec M1）。差的是把 `ChatLoop` 跑起来 —— 当前 `post_message` handler 直接写个 placeholder assistant 消息就返。三大 provider（OpenAI / Anthropic / OpenAi-compatible）协议有差异但都基于 `reqwest` + SSE 增量返回，封一个 adapter trait + 3 impl 是最直接的路。

## Goals / Non-Goals

**Goals:**
- 三个 provider 的 streaming chat completion 真实跑起来。
- ChatLoop 的 tool-call 回灌正确：拿到 tool_call → dispatcher → message log → 重新调 provider。
- 客户端拿到 SSE token stream（`event: chunk` / `event: tool` / `event: done` / `event: error`）。
- Cost 按 model_prices 算并落盘。

**Non-Goals:**
- 不实装客户端 SDK / UI 端的 SSE 消费（前端任务）。
- 不实装 image / file / vision modality（v1 仅文本）。
- 不实装 function-calling JSON schema 推断（前端传 ToolDispatcher 已知的 builtin_tools 即可）。
- 不实装 prompt caching / context truncation（先把基础链路跑通，后续 PR）。

## Decisions

### D1：用 `eventsource-stream` 解析 SSE

替代方案：手写按行解析。拒：rfc 边界（`event:` / `data:` / 多行 data 拼接 / `retry:` / 注释）易踩坑，`eventsource-stream` 是 ~200 LoC 的薄包。

### D2：`ChatLoop` trait 已在 enterprise crate，不动；新增 3 个 ProviderAdapter 文件

`enterprise/crates/copilot-chat/src/providers/{openai,anthropic,openai_compatible}.rs`。共享 `parse_chat_chunks(stream) -> impl Stream<...>` utility 提到 `providers/sse.rs`。

### D3：Tool call 序列化由 provider adapter 负责拼

OpenAI 把 tool_calls 分多个 SSE chunk 增量返回（每个 chunk 含 `index` + 部分 arguments JSON）。Adapter 内部 buffer 到 `finish_reason: tool_calls` 时再 yield 一个完整 `ChunkOrToolCall::ToolCall`。Anthropic 类似但是 `input_json_delta` event。把这部分粘连复杂度封在 adapter 内，ChatLoop 不需要关心。

### D4：Cost 在 assistant 消息持久化时计算

不在 stream 中实时算（增量 token 没用），等 provider 给出最终 usage 块再算。无 usage 信息时（少数 provider）记 NULL。

### D5：API key 走 env

`session.provider` 是逻辑名（`openai` / `anthropic` / `openai_compatible`），实际 HTTP credentials 通过 env var 注入，`MS_COPILOT_OPENAI_API_KEY` / `MS_COPILOT_ANTHROPIC_API_KEY` / `MS_COPILOT_COMPATIBLE_API_KEY` + `..._BASE_URL`。这样配置走标准 12-factor，避免 DB 存明文 key。

## Risks / Trade-offs

**[R1] 三个 provider 协议都在迭代，adapter 易过期**
→ Mitigation：mock test 覆盖每个 provider 的至少一种 streaming response shape；上游协议改动只动 adapter，ChatLoop 不变。

**[R2] SSE 长连接占 axum worker**
→ Mitigation：handler 已用 `Body::from_stream`，per-request 独立 task，hyper 自动 backpressure；不要做 buffering。

**[R3] tool dispatcher 慢拖慢 ChatLoop**
→ Mitigation：dispatcher 调用前 emit `event: tool_start`，调用后 `event: tool_end`，前端可显示 spinner；ChatLoop 不并行调多个 tool（OpenAI tool_calls.parallel 留 follow-up）。

**[R4] cost 计算漏算（特别是 system message）**
→ Mitigation：provider 返回 `usage.prompt_tokens` 已包含 system + history + tool messages 全部；按 provider 给的 token 数算，不在客户端做 tokenizer。
