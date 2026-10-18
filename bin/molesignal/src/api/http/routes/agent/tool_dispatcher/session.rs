// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 只在一轮回答里有意义的工具：任务清单（`todo_write`）与向提问人提问（`ask_user`），以及
//! 「确认」执行模式的工具执行前的确认。
//!
//! 它们要能和提问人说话（[`RunInteraction`]）：后台执行的 Run 提供，它把清单、问题与确认卡写进
//! 对话并等回答。没有时清单只回一句已记下，问题让模型自己判断并说明假设，「确认」模式的工具
//! 照旧不能直接执行。

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    agent::{
        model::RiskLevel,
        runs::QuestionKind,
        tools::{BuiltinToolKind, ToolResult},
    },
    shared::{Error, Result},
};

/// 没有人可问时交给模型的结果。
const NO_ONE_TO_ASK: &str = "No one can answer questions here. Continue with your best judgment and say which assumption you made.";
/// 等不到回答时交给模型的结果。
const NO_ANSWER: &str = "The person who asked did not answer in time. Continue with your best judgment and say which assumption you made.";

/// 提问人怎么回的。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Reply {
    Answered {
        choices: Vec<String>,
        text: String,
    },
    Approved,
    Declined,
    /// 等太久，没有回答。
    TimedOut,
    /// 这一轮被停下了。
    Stopped,
}

/// 和发起这一轮的人说话。
#[async_trait]
pub(crate) trait RunInteraction: Send + Sync {
    /// 任务清单（整份）变了。
    async fn todo(&self, items: Value);
    /// 问一个问题，等到回答。
    async fn ask(&self, question: String, kind: QuestionKind, options: Vec<String>) -> Reply;
    /// 请确认执行一个工具，等到决定。
    async fn confirm(&self, tool: &str, arguments: &Value, risk: RiskLevel) -> Reply;
    /// 在对话里留一条提醒，例如把一个团队挂进了会话。
    async fn notice(&self, payload: Value);
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TodoArgs {
    items: Vec<TodoItem>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct TodoItem {
    content: String,
    status: TodoStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AskArgs {
    question: String,
    #[serde(default)]
    kind: QuestionKind,
    #[serde(default)]
    options: Vec<String>,
}

fn parse<T: serde::de::DeserializeOwned>(arguments: Value) -> Result<T> {
    serde_json::from_value(arguments)
        .map_err(|error| Error::invalid(format!("invalid tool arguments: {error}")))
}

pub(super) async fn execute(
    interaction: Option<&dyn RunInteraction>,
    kind: BuiltinToolKind,
    arguments: Value,
) -> Result<ToolResult> {
    match kind {
        BuiltinToolKind::TodoWrite => {
            let args: TodoArgs = parse(arguments)?;

            if args.items.len() > 20 {
                return Ok(ToolResult::error("a task list holds at most 20 steps"));
            }

            let done = args
                .items
                .iter()
                .filter(|item| item.status == TodoStatus::Completed)
                .count();

            let total = args.items.len();

            if let Some(interaction) = interaction {
                interaction.todo(json!(args.items)).await;
            }

            Ok(ToolResult::json(
                json!({ "saved": true, "completed": done, "total": total }),
            ))
        }
        BuiltinToolKind::AskUser => {
            let args: AskArgs = parse(arguments)?;
            let question = args.question.trim().to_string();

            if question.is_empty() {
                return Ok(ToolResult::error("the question is empty"));
            }

            let options = args
                .options
                .into_iter()
                .map(|option| option.trim().to_string())
                .filter(|option| !option.is_empty())
                .take(8)
                .collect::<Vec<_>>();

            // 要选的问题没有选项时只能写回答。
            let kind = if options.is_empty() {
                QuestionKind::Text
            } else {
                args.kind
            };

            let Some(interaction) = interaction else {
                return Ok(ToolResult::json(
                    json!({ "answered": false, "note": NO_ONE_TO_ASK }),
                ));
            };

            Ok(match interaction.ask(question, kind, options).await {
                Reply::Answered { choices, text } => ToolResult::json(
                    json!({ "answered": true, "choices": choices, "answer": text }),
                ),
                Reply::Stopped => {
                    ToolResult::error("the run was stopped before the question was answered")
                }
                _ => ToolResult::json(json!({ "answered": false, "note": NO_ANSWER })),
            })
        }
        _ => Ok(ToolResult::error(format!("unknown tool: {}", kind.name()))),
    }
}

/// 「确认」模式的工具执行前问一句。返回 `Ok(())` 表示可以执行；否则是交给模型的结果。
pub(super) async fn confirmation(
    interaction: Option<&dyn RunInteraction>,
    tool: &str,
    arguments: &Value,
    risk: RiskLevel,
) -> std::result::Result<(), ToolResult> {
    let Some(interaction) = interaction else {
        return Err(ToolResult::error(format!(
            "tool `{tool}` requires confirmation and cannot execute directly"
        )));
    };

    match interaction.confirm(tool, arguments, risk).await {
        Reply::Approved => Ok(()),
        Reply::Declined => Err(ToolResult::error(format!(
            "the person who asked declined to run `{tool}`; do not run it again unless they ask"
        ))),
        Reply::Stopped => Err(ToolResult::error("the run was stopped")),
        _ => Err(ToolResult::error(format!(
            "no one confirmed `{tool}` in time, so it was not run"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use parking_lot::Mutex;

    use super::*;

    #[derive(Default)]
    struct Recorder {
        todos: Mutex<Vec<Value>>,
        reply: Option<Reply>,
    }

    #[async_trait]
    impl RunInteraction for Recorder {
        async fn todo(&self, items: Value) {
            self.todos.lock().push(items);
        }

        async fn ask(&self, _: String, kind: QuestionKind, options: Vec<String>) -> Reply {
            assert!(options.is_empty() || kind != QuestionKind::Text);
            self.reply.clone().unwrap_or(Reply::TimedOut)
        }

        async fn confirm(&self, _: &str, _: &Value, _: RiskLevel) -> Reply {
            self.reply.clone().unwrap_or(Reply::TimedOut)
        }

        async fn notice(&self, _: Value) {}
    }

    #[tokio::test]
    async fn the_task_list_is_counted_and_shown() {
        let recorder = Recorder::default();

        let result = execute(
            Some(&recorder),
            BuiltinToolKind::TodoWrite,
            json!({"items": [
                {"content": "Read the error logs", "status": "completed"},
                {"content": "Compare with the last deploy", "status": "in_progress"},
            ]}),
        )
        .await
        .unwrap();

        assert!(!result.is_error);
        assert!(result.first_text().unwrap().contains("\"completed\":1"));
        assert_eq!(recorder.todos.lock()[0][1]["status"], "in_progress");

        let wrong = execute(
            None,
            BuiltinToolKind::TodoWrite,
            json!({"items": [{"content": "x", "status": "done"}]}),
        )
        .await;
        assert!(wrong.is_err());
    }

    #[tokio::test]
    async fn a_question_without_anyone_to_answer_asks_the_model_to_decide() {
        let alone = execute(
            None,
            BuiltinToolKind::AskUser,
            json!({"question": "Which region?"}),
        )
        .await
        .unwrap();
        assert!(alone.first_text().unwrap().contains("best judgment"));

        let recorder = Recorder {
            reply: Some(Reply::Answered {
                choices: vec!["eu-west-1".into()],
                text: String::new(),
            }),
            ..Recorder::default()
        };

        let answered = execute(
            Some(&recorder),
            BuiltinToolKind::AskUser,
            json!({"question": "Which region?", "kind": "single", "options": ["eu-west-1", "us-east-1"]}),
        )
        .await
        .unwrap();

        assert!(answered.first_text().unwrap().contains("eu-west-1"));
    }

    #[tokio::test]
    async fn only_an_approval_lets_a_confirmation_tool_run() {
        let approve = Recorder {
            reply: Some(Reply::Approved),
            ..Recorder::default()
        };
        let decline = Recorder {
            reply: Some(Reply::Declined),
            ..Recorder::default()
        };

        assert!(
            confirmation(Some(&approve), "restart", &json!({}), RiskLevel::L1)
                .await
                .is_ok()
        );

        let declined = confirmation(Some(&decline), "restart", &json!({}), RiskLevel::L1)
            .await
            .unwrap_err();
        assert!(declined.first_text().unwrap().contains("declined"));
        assert!(
            confirmation(None, "restart", &json!({}), RiskLevel::L1)
                .await
                .is_err()
        );
    }
}
