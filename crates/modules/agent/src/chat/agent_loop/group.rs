// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The tool calls of one model reply, in the groups they run in. A group is a single call, or a
//! run of consecutive calls the dispatcher says only read ([`ToolDispatcher::runs_alongside`]):
//! those run together, at most [`MAX_CALLS_AT_ONCE`] at a time. Groups run one after another,
//! and results always come back in the order the model asked for the calls.

use std::ops::Range;

use futures::{Stream, StreamExt, stream};
use tool_runtime::{ToolCall, ToolResult};

use super::{
    budget::{Budget, Exhaustion},
    tool_failure_result,
};
use crate::{
    chat::RequestedToolCall,
    tools::{ToolAuthContext, ToolDispatcher},
};

/// How many calls of one group run at the same time.
pub(super) const MAX_CALLS_AT_ONCE: usize = 4;

/// What happens to one call of a group: it runs, or the budget no longer allows it.
pub(super) enum Planned {
    Run(ToolCall),
    Skip(Exhaustion),
}

/// Splits the calls of one reply into the groups they run in, in order.
pub(super) fn groups(
    dispatcher: &dyn ToolDispatcher,
    calls: &[RequestedToolCall],
) -> Vec<Range<usize>> {
    let mut groups = Vec::new();
    let mut start = 0;

    while start < calls.len() {
        let mut end = start + 1;

        if runs_alongside(dispatcher, &calls[start]) {
            while end < calls.len() && runs_alongside(dispatcher, &calls[end]) {
                end += 1;
            }
        }

        groups.push(start..end);
        start = end;
    }

    groups
}

fn runs_alongside(dispatcher: &dyn ToolDispatcher, call: &RequestedToolCall) -> bool {
    dispatcher.runs_alongside(&tool_call(call))
}

fn tool_call(call: &RequestedToolCall) -> ToolCall {
    ToolCall {
        name: call.name.clone(),
        arguments: call.arguments.clone(),
    }
}

/// Takes the budget for each call of a group, in order, before any of them starts.
pub(super) fn plan(budget: &mut Budget, calls: &[RequestedToolCall]) -> Vec<Planned> {
    calls
        .iter()
        .map(|call| match budget.take() {
            Ok(()) => Planned::Run(tool_call(call)),
            Err(reason) => Planned::Skip(reason),
        })
        .collect()
}

/// How many calls of a plan run.
pub(super) fn runs(planned: &[Planned]) -> usize {
    planned
        .iter()
        .filter(|planned| matches!(planned, Planned::Run(_)))
        .count()
}

/// The results of a planned group, in the calls' order. The calls that run start together, at
/// most [`MAX_CALLS_AT_ONCE`] at a time; a skipped call gets its result without running.
pub(super) fn run<'a>(
    dispatcher: &'a dyn ToolDispatcher,
    ctx: &'a ToolAuthContext,
    planned: Vec<Planned>,
) -> impl Stream<Item = ToolResult> + Send + 'a {
    stream::iter(planned)
        .map(move |planned| async move {
            match planned {
                Planned::Run(call) => dispatcher
                    .dispatch(ctx, call)
                    .await
                    .unwrap_or_else(tool_failure_result),
                Planned::Skip(reason) => reason.skipped_result(),
            }
        })
        .buffered(MAX_CALLS_AT_ONCE)
}

/// The result given for a call when its group ended without one.
pub(super) fn missing_result() -> ToolResult {
    ToolResult::error("the tool call returned no result")
}
