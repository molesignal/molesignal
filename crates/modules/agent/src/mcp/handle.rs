// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The server's prefix (handle) within its organization is generated from its name, contains only
//! lowercase letters, digits, and `_`, starts with a letter, is unique within the organization, and
//! stays unchanged when renamed. Tools are named `mcp__<handle>__<tool>` in the Agent (at most 64
//! characters, the model API's limit for tool names), and prompt commands are `/<handle>:<prompt>`.

use std::collections::HashSet;

const MAX_HANDLE: usize = 20;
const MAX_TOOL_NAME: usize = 64;
const MAX_PROMPT_NAME: usize = 48;

/// Keeps lowercase ASCII letters and digits, replaces consecutive other characters with a single
/// separator, and removes leading and trailing separators.
fn slug(value: &str, keep: &[char], max: usize) -> String {
    let mut result = String::new();
    let mut separated = false;

    for character in value.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() || keep.contains(&character) {
            result.push(character);
            separated = false;
        } else if !separated {
            result.push('_');
            separated = true;
        }
    }

    result
        .trim_matches('_')
        .chars()
        .take(max)
        .collect::<String>()
        .trim_end_matches('_')
        .to_string()
}

/// The prefix generated from a server name, before deduplication.
/// Uses `mcp` when the name has no usable letters (for example, when it is entirely Chinese).
pub fn handle_base(name: &str) -> String {
    let slug = slug(name, &[], MAX_HANDLE);

    if slug.starts_with(|c: char| c.is_ascii_lowercase()) {
        slug
    } else if slug.is_empty() {
        "mcp".into()
    } else {
        format!("mcp_{slug}")
            .chars()
            .take(MAX_HANDLE)
            .collect::<String>()
            .trim_end_matches('_')
            .to_string()
    }
}

/// The first available name among `candidate`, `candidate_2`, `candidate_3`, and so on, with a
/// length no greater than `max`.
fn first_free(candidate: &str, taken: &HashSet<String>, max: usize) -> String {
    if !taken.contains(candidate) {
        return candidate.to_string();
    }

    (2..)
        .map(|n| {
            let suffix = format!("_{n}");
            let room = max.saturating_sub(suffix.len());
            let head: String = candidate.chars().take(room).collect();

            format!("{}{suffix}", head.trim_end_matches('_'))
        })
        .find(|name| !taken.contains(name))
        .unwrap_or_default()
}

/// A prefix not yet used in the organization.
pub fn unique_handle(name: &str, taken: &HashSet<String>) -> String {
    first_free(&handle_base(name), taken, MAX_HANDLE)
}

/// The tool's name in the Agent; if names on the same server collide after normalization, later
/// ones get a sequence number.
pub fn tool_registry_name(handle: &str, remote: &str, taken: &HashSet<String>) -> String {
    let prefix = format!("mcp__{handle}__");
    let room = MAX_TOOL_NAME.saturating_sub(prefix.len()).max(8);
    let tool = slug(remote, &[], room);
    let tool = if tool.is_empty() { "tool".into() } else { tool };

    first_free(&format!("{prefix}{tool}"), taken, MAX_TOOL_NAME)
}

/// The prompt's command name (without `/`).
pub fn prompt_command(handle: &str, prompt: &str) -> String {
    let name = slug(prompt, &['-'], MAX_PROMPT_NAME);
    let name = if name.is_empty() {
        "prompt".into()
    } else {
        name
    };

    format!("{handle}:{name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_come_from_the_name() {
        assert_eq!(handle_base("Observability Server"), "observability_server");
        assert_eq!(handle_base("GitHub"), "github");
        assert_eq!(handle_base("监控平台"), "mcp");
        assert_eq!(handle_base("1Password"), "mcp_1password");
        assert_eq!(
            handle_base("a very long server name for testing"),
            "a_very_long_server_n"
        );
    }

    #[test]
    fn handles_are_unique_in_the_organization() {
        let taken = HashSet::from(["github".to_string(), "github_2".to_string()]);

        assert_eq!(unique_handle("GitHub", &taken), "github_3");
        assert_eq!(unique_handle("Linear", &taken), "linear");
    }

    #[test]
    fn tool_names_fit_model_limits_and_stay_unique() {
        let name = tool_registry_name("observability_server", "query.logs", &HashSet::new());
        assert_eq!(name, "mcp__observability_server__query_logs");

        let long = tool_registry_name(
            "a_very_long_server_n",
            "get_pull_request_review_comments_for_every_file",
            &HashSet::new(),
        );
        assert!(long.len() <= 64, "{long}");

        let taken = HashSet::from(["mcp__github__search".to_string()]);
        assert_eq!(
            tool_registry_name("github", "search!", &taken),
            "mcp__github__search_2"
        );
        assert_eq!(
            tool_registry_name("github", "查询", &HashSet::new()),
            "mcp__github__tool"
        );
    }

    #[test]
    fn prompt_commands_parse_as_commands() {
        assert_eq!(prompt_command("github", "Review PR"), "github:review_pr");
        assert_eq!(
            prompt_command("github", "summarize-issue"),
            "github:summarize-issue"
        );
        assert_eq!(prompt_command("github", "总结"), "github:prompt");

        match crate::commands::parse(&format!("/{} 42", prompt_command("github", "Review PR"))) {
            crate::commands::Input::Command(invocation) => {
                assert_eq!(invocation.name, "github:review_pr");
                assert_eq!(invocation.args, "42");
            }
            crate::commands::Input::Text(text) => panic!("read as text: {text}"),
        }
    }
}
