// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Run 上记下的请求：发消息时已经核对、展开好的东西，执行时照着做。

use serde::{Deserialize, Serialize};

use super::super::PostMessageReq;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum RunRequest {
    /// 回答一条消息。
    Answer(Box<AnswerRequest>),
    /// 把到现在为止的对话压缩成摘要（`/compact`）。
    Compact,
    /// 会话结束后提炼记忆：上一次提炼到 `since_micros`。
    Distill {
        #[serde(default)]
        since_micros: Option<i64>,
    },
}

impl RunRequest {
    /// `answer`、`compact` 或 `distill`：界面只把回答显示在对话里。
    pub(crate) fn kind_of(request: &serde_json::Value) -> &str {
        request
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("answer")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AnswerRequest {
    /// 这条消息的请求；是命令时已经按命令改写了模式与时间。
    pub(crate) message: PostMessageReq,
    pub(crate) turn: TurnSnapshot,
    /// 重新生成：历史截到这条用户消息，之前的回答不交给模型。
    #[serde(default)]
    pub(crate) regenerate: bool,
    /// 这一轮在写看板：输入是否已经齐全。
    #[serde(default)]
    pub(crate) dashboard: Option<bool>,
    /// 这一轮问题的时间：回答排在它后面。
    #[serde(default)]
    pub(crate) asked_at_micros: Option<i64>,
}

/// 命令展开后执行时还要用的部分。交给模型的问题记在用户消息上，不在这里。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct TurnSnapshot {
    /// 命令带来的 system 指令（例如事件的上下文）。
    #[serde(default)]
    pub(crate) system_texts: Vec<String>,
    /// 任务 prompt 里的 `alert_name`。
    #[serde(default)]
    pub(crate) alert_name: Option<String>,
    /// 这一轮直接交给模型的工具（Skill 的 `allowed-tools`）。
    #[serde(default)]
    pub(crate) pinned_tools: Vec<String>,
    /// 显式调用的 Skill，记进用量。
    #[serde(default)]
    pub(crate) skill_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_request_reads_back_what_was_queued() {
        let request = RunRequest::Answer(Box::new(AnswerRequest {
            message: serde_json::from_value(json!({
                "content": "/rca inc-1",
                "analysis_mode": "root_cause",
                "time_range": {"start_micros": 1, "end_micros": 2},
            }))
            .unwrap(),
            turn: TurnSnapshot {
                system_texts: vec!["incident facts".into()],
                alert_name: Some("API 502s".into()),
                ..TurnSnapshot::default()
            },
            regenerate: false,
            dashboard: None,
            asked_at_micros: Some(1),
        }));

        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(value["kind"], "answer");

        let RunRequest::Answer(read) = serde_json::from_value(value).unwrap() else {
            panic!("an answer request");
        };

        assert_eq!(read.message.analysis_mode.as_deref(), Some("root_cause"));
        assert_eq!(read.message.time_range.unwrap().end_micros, 2);
        assert_eq!(read.turn.alert_name.as_deref(), Some("API 502s"));

        let compact = serde_json::to_value(RunRequest::Compact).unwrap();
        assert_eq!(compact, json!({"kind": "compact"}));
    }
}
