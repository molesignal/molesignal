// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 流式运行：把模型写出的文字、工具调用与结果以 [`AgentStreamEvent`] 交给 [`AgentSink`]。
//!
//! 被停下（接收端说已取消）时，正在进行的模型调用或工具被打断，循环不再调用模型和工具，
//! 也不再花额度。[`AgentLoop::run_stream`] 把事件放进一个通道，调用方丢掉通道即停下；
//! [`AgentLoop::run_with_sink`] 在当前任务里跑，供后台执行的 Run 使用。

use std::{sync::Arc, time::Instant};

use async_trait::async_trait;
use futures::StreamExt;
use tokio::sync::mpsc::Sender;

use super::{
    AgentLoop, Budget, Exhaustion, INVALID_FINAL_RESPONSE_FALLBACK_MESSAGE,
    assistant_with_tool_calls, message, product_answer_or_fallback, tool_failure_result,
    tool_message,
};
use crate::agent::{
    chat::{
        AgentEventStream, AgentSink, AgentStreamEvent, ChatMessage, ChunkOrToolCall,
        CompletionRequest, MessageRole, RequestedToolCall, ToolChoice,
        model_call::{ModelCall, elapsed_ms},
    },
    tools::{ToolAuthContext, ToolCall},
};

/// 一次流式调用读完之后的结果。
struct StreamedReply {
    text: String,
    tool_calls: Vec<RequestedToolCall>,
    prompt_tokens: i32,
    cached_prompt_tokens: i32,
    completion_tokens: i32,
    finish_reason: Option<String>,
}

/// 一轮流式运行怎么结束的。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunConclusion {
    Answered {
        content: String,
        prompt_tokens: i32,
        completion_tokens: i32,
        finish_reason: String,
    },
    Failed(String),
    Cancelled,
}

/// 把事件放进通道的接收端：通道被丢掉就算停下。
struct ChannelSink {
    tx: Sender<AgentStreamEvent>,
}

#[async_trait]
impl AgentSink for ChannelSink {
    async fn emit(&self, event: AgentStreamEvent) {
        let _ = self.tx.send(event).await;
    }

    fn is_cancelled(&self) -> bool {
        self.tx.is_closed()
    }

    async fn cancelled(&self) {
        self.tx.closed().await;
    }
}

impl AgentLoop {
    /// 流式运行，事件从返回的流里读；丢掉这个流即停下。
    pub fn run_stream(
        self: Arc<Self>,
        ctx: ToolAuthContext,
        model: String,
        history: Vec<ChatMessage>,
        tools_schema: Option<serde_json::Value>,
        max_tokens: Option<i32>,
    ) -> AgentEventStream {
        self.run_stream_with_tool_choice(
            ctx,
            model,
            history,
            tools_schema,
            max_tokens,
            ToolChoice::Auto,
        )
    }

    pub fn run_stream_with_tool_choice(
        self: Arc<Self>,
        ctx: ToolAuthContext,
        model: String,
        history: Vec<ChatMessage>,
        tools_schema: Option<serde_json::Value>,
        max_tokens: Option<i32>,
        initial_tool_choice: ToolChoice,
    ) -> AgentEventStream {
        let (tx, rx) = tokio::sync::mpsc::channel::<AgentStreamEvent>(64);

        crate::shared::trace_context::spawn_with_current_trace_context(async move {
            let sink = ChannelSink { tx };

            self.run_with_sink(
                &sink,
                &ctx,
                &model,
                history,
                tools_schema,
                max_tokens,
                initial_tool_choice,
            )
            .await;
        });

        Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx))
    }

    /// 在当前任务里流式运行，事件交给 `sink`。最后一个事件是 `Done` 或 `Error`；被停下时
    /// 不再发事件。
    #[allow(clippy::too_many_arguments)]
    pub async fn run_with_sink(
        &self,
        sink: &dyn AgentSink,
        ctx: &ToolAuthContext,
        model: &str,
        history: Vec<ChatMessage>,
        tools_schema: Option<serde_json::Value>,
        max_tokens: Option<i32>,
        initial_tool_choice: ToolChoice,
    ) -> RunConclusion {
        let mut messages = history;
        let mut total_prompt = 0i32;
        let mut total_completion = 0i32;
        let mut budget = Budget::start(&self.limits);
        let mut tool_choice = initial_tool_choice;

        let exhaustion = loop {
            if sink.is_cancelled() {
                return RunConclusion::Cancelled;
            }

            if let Some(reason) = budget.exhausted() {
                break reason;
            }

            let req = self.request(
                model,
                messages.clone(),
                tools_schema.clone(),
                std::mem::replace(&mut tool_choice, ToolChoice::Auto),
                max_tokens,
            );

            if let Err(error) = req.validate_tool_choice() {
                return fail(sink, error.to_string()).await;
            }

            let reply = match self.stream_once(req, sink, true).await {
                Ok(reply) => reply,
                Err(_) if sink.is_cancelled() => return RunConclusion::Cancelled,
                Err(error) => return fail(sink, error).await,
            };

            total_prompt += reply.prompt_tokens;
            total_completion += reply.completion_tokens;

            if reply.tool_calls.is_empty() {
                let content = product_answer_or_fallback(
                    &reply.text,
                    INVALID_FINAL_RESPONSE_FALLBACK_MESSAGE,
                );

                return answer(
                    sink,
                    content,
                    total_prompt,
                    total_completion,
                    reply.finish_reason.unwrap_or_else(|| "stop".into()),
                )
                .await;
            }

            messages.push(assistant_with_tool_calls(reply.text, &reply.tool_calls));

            // 跑 tools 一遍并把结果作为 Tool message 追加；额度不够的调用不执行，但同样回一条
            // 结果并结束它在界面上的"进行中"。中间轮次的说明只回灌给模型，不进入回答。
            for tc in &reply.tool_calls {
                if sink.is_cancelled() {
                    return RunConclusion::Cancelled;
                }

                let result = match budget.take() {
                    Ok(()) => {
                        let call = ToolCall {
                            name: tc.name.clone(),
                            arguments: tc.arguments.clone(),
                        };

                        let dispatched = tokio::select! {
                            biased;
                            () = sink.cancelled() => return RunConclusion::Cancelled,
                            dispatched = self.dispatcher.dispatch(ctx, call) => dispatched,
                        };

                        dispatched.unwrap_or_else(tool_failure_result)
                    }
                    Err(reason) => reason.skipped_result(),
                };

                let (tool_result, result_json) = tool_message(&tc.id, &result);

                sink.emit(AgentStreamEvent::ToolEnd {
                    id: tc.id.clone(),
                    result: result_json,
                    is_error: result.is_error,
                })
                .await;

                messages.push(tool_result);
            }
        };

        self.finish(
            sink,
            model,
            messages,
            max_tokens,
            exhaustion,
            total_prompt,
            total_completion,
        )
        .await
    }

    /// 额度用完：不带工具再问一次，让模型用已有结果收尾。
    #[allow(clippy::too_many_arguments)]
    async fn finish(
        &self,
        sink: &dyn AgentSink,
        model: &str,
        mut messages: Vec<ChatMessage>,
        max_tokens: Option<i32>,
        exhaustion: Exhaustion,
        mut total_prompt: i32,
        mut total_completion: i32,
    ) -> RunConclusion {
        if sink.is_cancelled() {
            return RunConclusion::Cancelled;
        }

        messages.push(message(
            MessageRole::System,
            exhaustion.final_prompt().into(),
        ));

        let req = self.request(model, messages, None, ToolChoice::None, max_tokens);

        // 收尾失败时用兜底说明代替，不把内部预算错误暴露给用户。
        let final_text = match self.stream_once(req, sink, false).await {
            Ok(reply) => {
                total_prompt += reply.prompt_tokens;
                total_completion += reply.completion_tokens;
                reply.text
            }
            Err(_) if sink.is_cancelled() => return RunConclusion::Cancelled,
            Err(_) => String::new(),
        };

        let content = product_answer_or_fallback(&final_text, exhaustion.fallback());

        answer(
            sink,
            content,
            total_prompt,
            total_completion,
            exhaustion.finish_reason().into(),
        )
        .await
    }

    /// 流式调用一次模型服务并读完。`report` 时把写出的文字与读到的工具调用立即交给接收端；
    /// 收尾那次不需要。被停下或读的过程中出错时返回错误文本。
    async fn stream_once(
        &self,
        request: CompletionRequest,
        sink: &dyn AgentSink,
        report: bool,
    ) -> Result<StreamedReply, String> {
        let model = request.model.clone();
        let started = Instant::now();

        let mut reply = StreamedReply {
            text: String::new(),
            tool_calls: Vec::new(),
            prompt_tokens: 0,
            cached_prompt_tokens: 0,
            completion_tokens: 0,
            finish_reason: None,
        };

        let opened = tokio::select! {
            biased;
            () = sink.cancelled() => Err(crate::shared::Error::cancelled("the run was stopped")),
            opened = self.provider.complete_stream(request) => opened,
        };

        let outcome = match opened {
            Ok(mut stream) => loop {
                let next = tokio::select! {
                    biased;
                    () = sink.cancelled() => break Err("the run was stopped".to_string()),
                    next = stream.next() => next,
                };

                match next {
                    None => break Ok(()),
                    Some(Ok(ChunkOrToolCall::Text(text))) => {
                        if report && !text.is_empty() {
                            sink.emit(AgentStreamEvent::Delta(text.clone())).await;
                        }

                        reply.text.push_str(&text);
                    }
                    Some(Ok(ChunkOrToolCall::ToolCall {
                        id,
                        name,
                        arguments,
                    })) => {
                        if report {
                            sink.emit(AgentStreamEvent::ToolStart {
                                id: id.clone(),
                                name: name.clone(),
                                arguments: arguments.clone(),
                            })
                            .await;
                        }

                        reply.tool_calls.push(RequestedToolCall {
                            id,
                            name,
                            arguments: serde_json::from_str(&arguments)
                                .unwrap_or(serde_json::Value::String(arguments)),
                        });
                    }
                    Some(Ok(ChunkOrToolCall::Usage {
                        prompt_tokens,
                        cached_prompt_tokens,
                        completion_tokens,
                    })) => {
                        reply.prompt_tokens += prompt_tokens;
                        reply.cached_prompt_tokens += cached_prompt_tokens;
                        reply.completion_tokens += completion_tokens;
                    }
                    Some(Ok(ChunkOrToolCall::Done(reason))) => reply.finish_reason = Some(reason),
                    Some(Err(error)) => break Err(error.to_string()),
                }
            },
            Err(error) => Err(error.to_string()),
        };

        self.observe(ModelCall {
            provider: self.provider.provider(),
            model,
            prompt_tokens: i64::from(reply.prompt_tokens),
            cached_prompt_tokens: i64::from(reply.cached_prompt_tokens),
            completion_tokens: i64::from(reply.completion_tokens),
            latency_ms: elapsed_ms(started),
            streaming: true,
            tool_calls: reply.tool_calls.len(),
            finish_reason: reply.finish_reason.clone(),
            error: outcome.as_ref().err().cloned(),
        })
        .await;

        outcome.map(|()| reply)
    }
}

async fn fail(sink: &dyn AgentSink, error: String) -> RunConclusion {
    sink.emit(AgentStreamEvent::Error(error.clone())).await;
    RunConclusion::Failed(error)
}

async fn answer(
    sink: &dyn AgentSink,
    content: String,
    prompt_tokens: i32,
    completion_tokens: i32,
    finish_reason: String,
) -> RunConclusion {
    sink.emit(AgentStreamEvent::Chunk(content.clone())).await;

    sink.emit(AgentStreamEvent::Done {
        content: content.clone(),
        prompt_tokens,
        completion_tokens,
        finish_reason: finish_reason.clone(),
    })
    .await;

    RunConclusion::Answered {
        content,
        prompt_tokens,
        completion_tokens,
        finish_reason,
    }
}
