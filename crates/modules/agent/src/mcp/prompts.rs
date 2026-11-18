// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Prompts provided by MCP servers.
//!
//! Synchronization records names, descriptions, and arguments; the command menu displays them as
//! `/<handle>:<prompt>`. Text after the command fills the arguments: if every word uses
//! `name=value`, bind by name; otherwise, bind one word per argument in order, with the last
//! argument taking all remaining text (a single argument takes the entire text). Values containing
//! whitespace can be quoted.

use kernel::{ids::Id, time::TimestampMicros};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpPromptArgument {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpPrompt {
    pub org_id: Id,
    pub server_id: Id,
    /// The name on the server, used when calling `prompts/get`.
    pub name: String,
    pub title: Option<String>,
    pub description: String,
    pub arguments: Vec<McpPromptArgument>,
    pub synced_at: TimestampMicros,
}

impl McpPrompt {
    /// Argument hints in the menu: required arguments use `<name>`, optional ones use `[name]`.
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

/// Splits words on whitespace, preserves whitespace inside quotes, and removes the quotes
/// themselves.
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

/// Converts text after the command to prompt arguments. Returns the name of a missing required
/// argument.
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

/// Converts a `prompts/get` result to text passed to the model for this turn: collects text and
/// text from embedded resources in order, uses placeholders for images and similar content, and
/// prefixes assistant messages with `Assistant:`. Truncates anything beyond `max_bytes`.
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
