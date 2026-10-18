// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Run 的事件：按序号递增，浏览器从某个序号之后续读。
//!
//! 模型写出的文字一小段一小段地到，合并后再写：攒够 [`TEXT_FLUSH_BYTES`] 或攒了
//! [`TEXT_FLUSH_MILLIS`] 毫秒写一条；其他事件之前先把攒着的文字写掉，保证顺序。最后一个事件
//! 总是 [`RunEventKind::Done`]。

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::shared::{ids::Id, time::TimestampMicros};

/// 攒够这么多字节的文字就写一条。
pub const TEXT_FLUSH_BYTES: usize = 1024;
/// 文字最多攒这么久。
pub const TEXT_FLUSH_MILLIS: u64 = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunEventKind {
    /// 状态变了：开始执行、在等回答、在等确认、回到执行。
    Status,
    /// 模型写出的文字。一次调用以工具调用结束时，这些文字是中间的说明，不是回答。
    Text,
    ToolStart,
    ToolEnd,
    /// 任务清单（整份）。
    Todo,
    /// 向提问人提的问题。
    Question,
    /// 请提问人确认执行一个工具。
    Confirmation,
    /// 提问人的回答或决定。
    Response,
    /// 提醒：预算、上下文压缩……
    Notice,
    /// 结束：成功、失败、停止或中断。
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
            Self::Response,
            Self::Notice,
            Self::Done,
        ]
        .into_iter()
        .find(|kind| kind.as_str() == value)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunEvent {
    pub run_id: Id,
    pub seq: i64,
    pub kind: RunEventKind,
    pub payload: Value,
    pub created_at: TimestampMicros,
}

/// 还没分到序号的事件。
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

/// 攒着还没写的文字。
#[derive(Debug, Default)]
pub struct TextBuffer {
    text: String,
    since: Option<Instant>,
}

impl TextBuffer {
    /// 放进一段文字；攒够了就把全部取出来。
    pub fn push(&mut self, delta: &str) -> Option<String> {
        if delta.is_empty() {
            return None;
        }

        self.since.get_or_insert_with(Instant::now);
        self.text.push_str(delta);

        (self.text.len() >= TEXT_FLUSH_BYTES).then(|| self.take())
    }

    /// 攒得够久了。
    pub fn due(&self, now: Instant) -> bool {
        self.since.is_some_and(|since| {
            now.duration_since(since) >= Duration::from_millis(TEXT_FLUSH_MILLIS)
        })
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// 取出攒着的全部文字。
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
