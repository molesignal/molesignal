// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Run events have increasing sequence numbers, and the browser resumes reading after a given
//! number.
//!
//! Text from the model arrives in small chunks and is combined before writing: one event is written
//! after accumulating [`TEXT_FLUSH_BYTES`] or waiting [`TEXT_FLUSH_MILLIS`] milliseconds. Buffered
//! text is written before other events to preserve order. The last event is always
//! [`RunEventKind::Done`].

use std::time::{Duration, Instant};

use kernel::{ids::Id, time::TimestampMicros};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Writes one event after accumulating this many bytes of text.
pub const TEXT_FLUSH_BYTES: usize = 1024;
/// The maximum time to buffer text.
pub const TEXT_FLUSH_MILLIS: u64 = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunEventKind {
    /// The status changes: execution starts, waits for an answer, waits for confirmation, or
    /// resumes.
    Status,
    /// Text written by the model.
    /// When a call ends with tool calls, this text is intermediate commentary, not an answer.
    Text,
    ToolStart,
    ToolEnd,
    /// The task list in full.
    Todo,
    /// A question for the requester.
    Question,
    /// Asks the requester to confirm running a tool.
    Confirmation,
    /// Asks the requester to save their credentials for an MCP server (`Authorize to continue`).
    Authorization,
    /// The requester's answer or decision.
    Response,
    /// A notice: budget, context compaction, etc.
    Notice,
    /// Completion: success, failure, stop, or interruption.
    Done,
}

impl RunEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Text => "text",
            Self::ToolStart => "tool_start",
            Self::ToolEnd => "tool_end",
            Self::Todo => "todo",
            Self::Question => "question",
            Self::Confirmation => "confirmation",
            Self::Authorization => "authorization",
            Self::Response => "response",
            Self::Notice => "notice",
            Self::Done => "done",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::Status,
            Self::Text,
            Self::ToolStart,
            Self::ToolEnd,
            Self::Todo,
            Self::Question,
            Self::Confirmation,
            Self::Authorization,
            Self::Response,
            Self::Notice,
            Self::Done,
        ]
        .into_iter()
        .find(|kind| kind.as_str() == value)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunEvent {
    pub run_id: Id,
    pub seq: i64,
    pub kind: RunEventKind,
    pub payload: Value,
    pub created_at: TimestampMicros,
}

/// An event that has not yet been assigned a sequence number.
#[derive(Debug, Clone, PartialEq)]
pub struct NewRunEvent {
    pub kind: RunEventKind,
    pub payload: Value,
}

impl NewRunEvent {
    pub fn new(kind: RunEventKind, payload: Value) -> Self {
        Self { kind, payload }
    }
}

/// Buffered text that has not yet been written.
#[derive(Debug, Default)]
pub struct TextBuffer {
    text: String,
    since: Option<Instant>,
}

impl TextBuffer {
    /// Adds a chunk of text and takes out all buffered text when enough has accumulated.
    pub fn push(&mut self, delta: &str) -> Option<String> {
        if delta.is_empty() {
            return None;
        }

        self.since.get_or_insert_with(Instant::now);
        self.text.push_str(delta);

        (self.text.len() >= TEXT_FLUSH_BYTES).then(|| self.take())
    }

    /// Text has been buffered long enough.
    pub fn due(&self, now: Instant) -> bool {
        self.since.is_some_and(|since| {
            now.duration_since(since) >= Duration::from_millis(TEXT_FLUSH_MILLIS)
        })
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Takes out all buffered text.
    pub fn take(&mut self) -> String {
        self.since = None;
        std::mem::take(&mut self.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_round_trip() {
        for kind in [
            RunEventKind::Status,
            RunEventKind::Text,
            RunEventKind::ToolStart,
            RunEventKind::ToolEnd,
            RunEventKind::Todo,
            RunEventKind::Question,
            RunEventKind::Confirmation,
            RunEventKind::Authorization,
            RunEventKind::Response,
            RunEventKind::Notice,
            RunEventKind::Done,
        ] {
            assert_eq!(RunEventKind::parse(kind.as_str()), Some(kind));
        }
    }

    #[test]
    fn text_is_written_once_enough_has_gathered_or_it_has_waited() {
        let mut buffer = TextBuffer::default();

        assert_eq!(buffer.push("hello "), None);
        assert!(!buffer.due(Instant::now()));
        assert!(buffer.due(Instant::now() + Duration::from_millis(TEXT_FLUSH_MILLIS)));

        let long = "x".repeat(TEXT_FLUSH_BYTES);
        assert_eq!(
            buffer.push(&long).map(|text| text.len()),
            Some(6 + TEXT_FLUSH_BYTES)
        );
        assert!(buffer.is_empty());
        assert!(!buffer.due(Instant::now() + Duration::from_secs(1)));
    }
}
