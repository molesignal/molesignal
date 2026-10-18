// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! MCP 服务器提供的 prompt。同步时记下名称、说明与参数，命令菜单里写成 `/<handle>:<prompt>`。
//!
//! 调用时把命令后面的文字填进参数：每个词都写成 `名称=值` 时按名称填；否则按顺序一个词填
//! 一个参数，最后一个参数拿走剩下的全部文字（只有一个参数时就是整段文字）。值里有空白时
//! 可以加引号。

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::shared::{ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpPromptArgument {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpPrompt {
    pub org_id: Id,
    pub server_id: Id,
    /// 服务器上的名称，调用 `prompts/get` 用。
    pub name: String,
    pub title: Option<String>,
    pub description: String,
    pub arguments: Vec<McpPromptArgument>,
    pub synced_at: TimestampMicros,
}

impl McpPrompt {
    /// 菜单里的参数提示：必填的写 `<名称>`，选填的写 `[名称]`。
    pub fn argument_hint(&self) -> Option<String> {
        if self.arguments.is_empty() {
            return None;
        }

        Some(
            self.arguments
                .iter()
                .map(|argument| {
                    if argument.required {
                        format!("<{}>", argument.name)
                    } else {
                        format!("[{}]", argument.name)
                    }
                })
                .collect::<Vec<_>>()
                .join(" "),
        )
    }
}

/// 按空白切词，引号里的空白不切；引号本身去掉。
fn words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;

    for character in text.chars() {
        match quote {
            Some(open) if character == open => quote = None,
            Some(_) => current.push(character),
            None if character == '"' || character == '\'' => {
                quote = Some(character);
                started = true;
            }
            None if character.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            None => {
                current.push(character);
                started = true;
            }
        }
    }

    if started {
        words.push(current);
    }

    words
}

/// 命令后的文字 → prompt 参数。缺了必填参数时返回它的名称。
pub fn bind_arguments(
    arguments: &[McpPromptArgument],
    text: &str,
) -> std::result::Result<Map<String, Value>, String> {
    let text = text.trim();
    let mut bound = Map::new();

    let named = words(text);

    let all_named = !named.is_empty()
        && named.iter().all(|word| {
            word.split_once('=')
                .is_some_and(|(key, _)| arguments.iter().any(|argument| argument.name == key))
        });

    if all_named {
        for word in named {
            if let Some((key, value)) = word.split_once('=') {
                bound.insert(key.to_string(), Value::String(value.to_string()));
            }
        }
    } else if arguments.len() == 1 && !text.is_empty() {
        bound.insert(arguments[0].name.clone(), Value::String(text.to_string()));
    } else if !text.is_empty() {
        let mut words = named.into_iter();

        for (position, argument) in arguments.iter().enumerate() {
            let value = if position + 1 == arguments.len() {
                words.by_ref().collect::<Vec<_>>().join(" ")
            } else {
                words.next().unwrap_or_default()
            };

            if !value.is_empty() {
                bound.insert(argument.name.clone(), Value::String(value));
            }
        }
    }

    match arguments
        .iter()
        .find(|argument| argument.required && !bound.contains_key(&argument.name))
    {
        Some(missing) => Err(missing.name.clone()),
        None => Ok(bound),
    }
}

fn content_text(content: &Value) -> Option<String> {
    match content["type"].as_str()? {
        "text" => content["text"].as_str().map(str::to_string),
        "resource" => content["resource"]["text"]
            .as_str()
            .map(str::to_string)
            .or_else(|| {
                content["resource"]["uri"]
                    .as_str()
                    .map(|uri| format!("[resource {uri}]"))
            }),
        "resource_link" => content["uri"]
            .as_str()
            .map(|uri| format!("[resource {uri}]")),
        other => Some(format!("[{other} omitted]")),
    }
}

/// `prompts/get` 的结果 → 这一轮交给模型的文字：按顺序取文本与内嵌资源里的文本，图片等写一个
/// 占位；助手角色的消息前面标上 `Assistant:`。超过 `max_bytes` 的截掉。
pub fn prompt_text(result: &Value, max_bytes: usize) -> String {
    let mut parts = Vec::new();

    for message in result["messages"].as_array().into_iter().flatten() {
        let contents = match &message["content"] {
            Value::Array(items) => items.iter().collect::<Vec<_>>(),
            Value::Null => Vec::new(),
            single => vec![single],
        };

        let text = contents
            .into_iter()
            .filter_map(content_text)
            .collect::<Vec<_>>()
            .join("\n");

        if text.trim().is_empty() {
            continue;
        }

        if message["role"].as_str() == Some("assistant") {
            parts.push(format!("Assistant: {text}"));
        } else {
            parts.push(text);
        }
    }

    let mut text = parts.join("\n\n");

    if text.len() > max_bytes {
        let mut end = max_bytes;

        while !text.is_char_boundary(end) {
            end -= 1;
        }

        text.truncate(end);
    }

    text
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn argument(name: &str, required: bool) -> McpPromptArgument {
        McpPromptArgument {
            name: name.into(),
            description: String::new(),
            required,
        }
    }

    #[test]
    fn one_argument_takes_the_whole_text() {
        let bound = bind_arguments(&[argument("question", true)], "why is checkout slow").unwrap();

        assert_eq!(bound["question"], "why is checkout slow");
    }

    #[test]
    fn positional_words_fill_in_order_and_the_last_takes_the_rest() {
        let arguments = [
            argument("repo", true),
            argument("pr", true),
            argument("focus", false),
        ];
        let bound = bind_arguments(&arguments, "acme/api 42 error handling and tests").unwrap();

        assert_eq!(bound["repo"], "acme/api");
        assert_eq!(bound["pr"], "42");
        assert_eq!(bound["focus"], "error handling and tests");

        assert_eq!(
            bind_arguments(&arguments, "acme/api"),
            Err("pr".to_string())
        );
    }

    #[test]
    fn named_values_and_quotes_are_understood() {
        let arguments = [argument("repo", true), argument("pr", false)];
        let bound = bind_arguments(&arguments, r#"pr=7 repo="acme/web app""#).unwrap();

        assert_eq!(bound["repo"], "acme/web app");
        assert_eq!(bound["pr"], "7");

        assert!(bind_arguments(&[], "").unwrap().is_empty());
        assert_eq!(
            bind_arguments(&[argument("x", true)], " "),
            Err("x".to_string())
        );
    }

    #[test]
    fn prompt_messages_become_one_text() {
        let result = json!({
            "description": "Review a pull request",
            "messages": [
                {"role": "user", "content": {"type": "text", "text": "Review PR 42."}},
                {"role": "user", "content": {"type": "resource", "resource": {"uri": "file:///diff", "text": "+fix"}}},
                {"role": "assistant", "content": {"type": "text", "text": "Sure."}},
                {"role": "user", "content": {"type": "image", "data": "...", "mimeType": "image/png"}}
            ]
        });

        assert_eq!(
            prompt_text(&result, 10_000),
            "Review PR 42.\n\n+fix\n\nAssistant: Sure.\n\n[image omitted]"
        );
        assert_eq!(prompt_text(&result, 6), "Review");
        assert_eq!(prompt_text(&json!({"messages": []}), 100), "");
    }

    #[test]
    fn the_hint_marks_required_arguments() {
        let prompt = McpPrompt {
            org_id: Id::from_string("org"),
            server_id: Id::from_string("server"),
            name: "review".into(),
            title: None,
            description: String::new(),
            arguments: vec![argument("pr", true), argument("focus", false)],
            synced_at: TimestampMicros(1),
        };

        assert_eq!(prompt.argument_hint().as_deref(), Some("<pr> [focus]"));
    }
}
