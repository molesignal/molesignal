// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Tool calls of one reply that only read run together; everything else runs alone.

use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use futures::StreamExt;
use kernel::{Result, ids::Id, time::TimestampMicros};
use tokio::sync::Barrier;
use tool_runtime::{ToolCall, ToolContent, ToolResult};

use super::{AgentLoop, LoopLimits, group};
use crate::{
    chat::{
        AgentStreamEvent, ChatMessage, CompletionRequest, CompletionResponse, MessageRole,
        Provider, ProviderAdapter, RequestedToolCall,
    },
    tools::{ToolAuthContext, ToolDispatcher},
};

/// Answers each round with the next list of tool calls while there are tools to call, then
/// with a final text; keeps every request.
struct ScriptProvider {
    rounds: Mutex<VecDeque<Vec<RequestedToolCall>>>,
    requests: Mutex<Vec<CompletionRequest>>,
}

impl ScriptProvider {
    fn new(rounds: Vec<Vec<RequestedToolCall>>) -> Self {
        Self {
            rounds: Mutex::new(rounds.into()),
            requests: Mutex::new(Vec::new()),
        }
    }

    /// The tool results of the last request, in order: (call id, content).
    fn last_results(&self) -> Vec<(String, String)> {
        let requests = self.requests.lock().unwrap();

        requests
            .last()
            .map(|request| {
                request
                    .messages
                    .iter()
                    .filter(|message| message.role == MessageRole::Tool)
                    .map(|message| {
                        (
                            message.tool_call_id.clone().unwrap_or_default(),
                            message.content.clone(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[async_trait]
impl ProviderAdapter for ScriptProvider {
    fn provider(&self) -> Provider {
        Provider::OpenAi
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        self.requests.lock().unwrap().push(req.clone());

        let calls = if req.tools.is_some() {
            self.rounds.lock().unwrap().pop_front()
        } else {
            None
        };

        Ok(match calls {
            Some(tool_calls) => CompletionResponse {
                content: None,
                tool_calls,
                prompt_tokens: 10,
                cached_prompt_tokens: 0,
                completion_tokens: 2,
                finish_reason: "tool_calls".into(),
            },
            None => CompletionResponse {
                content: Some("done".into()),
                tool_calls: Vec::new(),
                prompt_tokens: 10,
                cached_prompt_tokens: 0,
                completion_tokens: 2,
                finish_reason: "stop".into(),
            },
        })
    }
}

/// Calls named `read_*` only read. Each call waits `delayMs`, and at the barrier when there is
/// one; the dispatcher keeps how many calls ran and the most that ran at once.
#[derive(Default)]
struct ReadingDispatcher {
    barrier: Option<Barrier>,
    running: AtomicUsize,
    most_at_once: AtomicUsize,
    calls: AtomicUsize,
}

impl ReadingDispatcher {
    fn with_barrier(parties: usize) -> Self {
        Self {
            barrier: Some(Barrier::new(parties)),
            ..Self::default()
        }
    }
}

#[async_trait]
impl ToolDispatcher for ReadingDispatcher {
    async fn dispatch(&self, _ctx: &ToolAuthContext, call: ToolCall) -> Result<ToolResult> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let running = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.most_at_once.fetch_max(running, Ordering::SeqCst);

        if let Some(barrier) = &self.barrier {
            barrier.wait().await;
        }

        let delay = call.arguments["delayMs"].as_u64().unwrap_or(0);
        tokio::time::sleep(Duration::from_millis(delay)).await;
        self.running.fetch_sub(1, Ordering::SeqCst);

        Ok(ToolResult {
            content: vec![ToolContent::Text {
                text: format!("result of {}", call.arguments["tag"].as_str().unwrap_or("")),
            }],
            is_error: false,
        })
    }

    fn runs_alongside(&self, call: &ToolCall) -> bool {
        call.name.starts_with("read_")
    }
}

fn ctx() -> ToolAuthContext {
    ToolAuthContext::from_iam(&domain::iam::IamContext {
        user_id: Id("u1".into()),
        org_id: Id("orgA".into()),
        display_role: String::new(),
        roles: Vec::new(),
        credential_token_id: None,
        credential_application_id: None,
        credential_service_account_id: None,
        scope: domain::iam::IamScope::Organization,
        permissions: Default::default(),
        policy_version: 0,
    })
}

fn question() -> Vec<ChatMessage> {
    vec![ChatMessage {
        id: Id::new(),
        chat_id: Id("chat".into()),
        role: MessageRole::User,
        content: "what happened?".into(),
        tool_call_id: None,
        tool_calls: serde_json::Value::Null,
        created_at: TimestampMicros::now(),
        prompt_tokens: None,
        completion_tokens: None,
    }]
}

fn tools() -> Option<serde_json::Value> {
    Some(serde_json::json!([{"type": "function", "function": {"name": "read_logs"}}]))
}

fn call(index: usize, name: &str, delay_ms: u64) -> RequestedToolCall {
    RequestedToolCall {
        id: format!("tc-{index}"),
        name: name.into(),
        arguments: serde_json::json!({"tag": format!("c{index}"), "delayMs": delay_ms}),
    }
}

#[test]
fn consecutive_reads_form_one_group_and_the_rest_run_alone() {
    let dispatcher = ReadingDispatcher::default();
    let calls = vec![
        call(0, "read_logs", 0),
        call(1, "read_metrics", 0),
        call(2, "write_rule", 0),
        call(3, "read_traces", 0),
        call(4, "read_logs", 0),
        call(5, "write_note", 0),
        call(6, "write_note", 0),
    ];

    assert_eq!(
        group::groups(&dispatcher, &calls),
        vec![0..2, 2..3, 3..5, 5..6, 6..7]
    );
}

#[tokio::test]
async fn reads_of_one_reply_run_together() {
    // Three calls wait for each other at the barrier: one at a time, the answer never comes.
    let provider = Arc::new(ScriptProvider::new(vec![vec![
        call(0, "read_logs", 0),
        call(1, "read_metrics", 0),
        call(2, "read_traces", 0),
    ]]));
    let dispatcher = Arc::new(ReadingDispatcher::with_barrier(3));
    let agent = AgentLoop::new(provider.clone(), dispatcher.clone());

    let result = tokio::time::timeout(
        Duration::from_secs(5),
        agent.run(&ctx(), "m".into(), question(), tools(), None),
    )
    .await
    .expect("the three reads ran together")
    .unwrap();

    assert_eq!(result.tool_calls_made, 3);
    assert_eq!(dispatcher.most_at_once.load(Ordering::SeqCst), 3);

    let ids = provider
        .last_results()
        .into_iter()
        .map(|(id, _)| id)
        .collect::<Vec<_>>();

    assert_eq!(ids, vec!["tc-0", "tc-1", "tc-2"]);
}

#[tokio::test]
async fn at_most_four_calls_run_at_once() {
    let provider = Arc::new(ScriptProvider::new(vec![
        (0..6).map(|index| call(index, "read_logs", 20)).collect(),
    ]));
    let dispatcher = Arc::new(ReadingDispatcher::default());

    AgentLoop::new(provider.clone(), dispatcher.clone())
        .with_limits(LoopLimits {
            max_tool_calls: 10,
            ..LoopLimits::default()
        })
        .run(&ctx(), "m".into(), question(), tools(), None)
        .await
        .unwrap();

    assert_eq!(dispatcher.calls.load(Ordering::SeqCst), 6);
    assert_eq!(
        dispatcher.most_at_once.load(Ordering::SeqCst),
        group::MAX_CALLS_AT_ONCE
    );
}

#[tokio::test]
async fn calls_that_do_more_than_read_run_one_at_a_time() {
    let provider = Arc::new(ScriptProvider::new(vec![vec![
        call(0, "write_rule", 10),
        call(1, "write_note", 10),
        call(2, "write_rule", 10),
    ]]));
    let dispatcher = Arc::new(ReadingDispatcher::default());

    AgentLoop::new(provider.clone(), dispatcher.clone())
        .run(&ctx(), "m".into(), question(), tools(), None)
        .await
        .unwrap();

    assert_eq!(dispatcher.calls.load(Ordering::SeqCst), 3);
    assert_eq!(dispatcher.most_at_once.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn results_follow_the_calls_and_not_who_finished_first() {
    // The first read is the slowest: its result still comes first.
    let provider = Arc::new(ScriptProvider::new(vec![vec![
        call(0, "read_logs", 40),
        call(1, "read_metrics", 20),
        call(2, "write_note", 0),
        call(3, "read_traces", 0),
    ]]));
    let dispatcher = Arc::new(ReadingDispatcher::default());

    AgentLoop::new(provider.clone(), dispatcher.clone())
        .run(&ctx(), "m".into(), question(), tools(), None)
        .await
        .unwrap();

    let results = provider.last_results();
    let ids = results
        .iter()
        .map(|(id, _)| id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(ids, vec!["tc-0", "tc-1", "tc-2", "tc-3"]);
    assert!(results[0].1.contains("result of c0"), "{}", results[0].1);
    assert!(results[3].1.contains("result of c3"), "{}", results[3].1);
}

#[tokio::test]
async fn streamed_results_are_told_in_the_order_of_the_calls() {
    let provider = Arc::new(ScriptProvider::new(vec![vec![
        call(0, "read_logs", 40),
        call(1, "read_metrics", 20),
        call(2, "read_traces", 0),
    ]]));
    let dispatcher = Arc::new(ReadingDispatcher::default());
    let agent = Arc::new(AgentLoop::new(provider.clone(), dispatcher.clone()));

    let mut events = agent.run_stream(ctx(), "m".into(), question(), tools(), None);
    let mut ended = Vec::new();
    let mut done = false;

    while let Some(event) = events.next().await {
        match event {
            AgentStreamEvent::ToolEnd { id, is_error, .. } => {
                assert!(!is_error);
                ended.push(id);
            }
            AgentStreamEvent::Done { .. } => done = true,
            AgentStreamEvent::Error(error) => panic!("unexpected error: {error}"),
            _ => {}
        }
    }

    assert!(done);
    assert_eq!(ended, vec!["tc-0", "tc-1", "tc-2"]);
    assert!(dispatcher.most_at_once.load(Ordering::SeqCst) > 1);
}

#[tokio::test]
async fn a_group_the_budget_cannot_cover_runs_only_what_it_can() {
    let provider = Arc::new(ScriptProvider::new(vec![vec![
        call(0, "read_logs", 0),
        call(1, "read_metrics", 0),
        call(2, "read_traces", 0),
    ]]));
    let dispatcher = Arc::new(ReadingDispatcher::default());

    let result = AgentLoop::new(provider.clone(), dispatcher.clone())
        .with_limits(LoopLimits {
            max_tool_calls: 2,
            ..LoopLimits::default()
        })
        .run(&ctx(), "m".into(), question(), tools(), None)
        .await
        .unwrap();

    assert_eq!(result.tool_calls_made, 2);
    assert_eq!(dispatcher.calls.load(Ordering::SeqCst), 2);
    assert_eq!(result.finish_reason, "tool_budget_exhausted");

    let results = provider.last_results();

    assert_eq!(results.len(), 3);
    assert!(results[1].1.contains("result of c1"), "{}", results[1].1);
    assert!(results[2].1.contains("Not executed"), "{}", results[2].1);
}
