// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Converts raw input to plain text or a command.
//! The rules match the frontend's `web/src/routes/agent/chat/composer/commands.ts`:
//!
//! - After removing leading whitespace, `/` followed by a segment that starts with a letter and
//!   contains only letters, digits, `_`, `-`, and `:` up to the first whitespace is a command;
//!   names are case-insensitive, and the remaining text, trimmed at both ends, is the argument.
//! - The name may be followed by `@team` (up to whitespace) to specify a team's Skill.
//! - If the segment contains other characters (such as `/var/log/app.log`), it is plain text.
//! - A leading `\/` is an escape: remove the backslash and treat the input as plain text.

use super::CommandRecord;

/// A command invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// The lowercase command name.
    pub name: String,
    /// The team spelling in `/name@team`.
    pub team: Option<String>,
    pub args: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// Plain text with the escape backslash already removed.
    Text(String),
    Command(Invocation),
}

pub fn parse(content: &str) -> Input {
    let trimmed = content.trim_start();

    if let Some(rest) = trimmed.strip_prefix("\\/") {
        return Input::Text(format!("/{rest}"));
    }

    let Some(body) = trimmed.strip_prefix('/') else {
        return Input::Text(content.to_string());
    };

    let (token, args) = match body.find(char::is_whitespace) {
        Some(at) => (&body[..at], body[at..].trim()),
        None => (body, ""),
    };

    let (name, team) = match token.split_once('@') {
        Some((name, team)) if !team.is_empty() && !team.contains('/') => (name, Some(team)),
        Some(_) => return Input::Text(content.to_string()),
        None => (token, None),
    };

    let mut chars = name.chars();
    let starts_with_letter = chars.next().is_some_and(|c| c.is_ascii_alphabetic());
    let rest_is_name = chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ':'));

    if !starts_with_letter || !rest_is_name {
        return Input::Text(content.to_string());
    }

    Input::Command(Invocation {
        name: name.to_ascii_lowercase(),
        team: team.map(str::to_string),
        args: args.to_string(),
    })
}

/// The text of a user message passed to the model: the recorded expanded question for a command,
/// the text with the escape backslash removed for escaped input, or the original text otherwise.
pub fn text_for_model(content: &str, command: Option<&CommandRecord>) -> String {
    if let Some(command) = command {
        return command.model_text.clone();
    }

    match parse(content) {
        Input::Text(text) => text,
        Input::Command(_) => content.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::CommandSource;

    fn command(name: &str, args: &str) -> Input {
        Input::Command(Invocation {
            name: name.into(),
            team: None,
            args: args.into(),
        })
    }

    #[test]
    fn a_leading_slash_and_a_name_make_a_command() {
        assert_eq!(parse("/help"), command("help", ""));
        assert_eq!(parse("  /Mode   deep "), command("mode", "deep"));
        assert_eq!(
            parse("/query why is\ncheckout slow"),
            command("query", "why is\ncheckout slow")
        );
        assert_eq!(parse("/github:review 42"), command("github:review", "42"));
        assert_eq!(
            parse("/runbook@SRE值班 db down"),
            Input::Command(Invocation {
                name: "runbook".into(),
                team: Some("SRE值班".into()),
                args: "db down".into(),
            })
        );
    }

    #[test]
    fn paths_and_bare_slashes_stay_text() {
        for text in [
            "/var/log/app.log has errors",
            "/ is the root",
            "/",
            "/1h ago",
            "/帮助",
            "why /help",
            "/runbook@ x",
            "/a@b/c",
        ] {
            assert_eq!(parse(text), Input::Text(text.to_string()), "{text}");
        }
    }

    #[test]
    fn a_backslash_sends_the_slash_as_text() {
        assert_eq!(parse("\\/help me"), Input::Text("/help me".into()));
        assert_eq!(parse("  \\/query"), Input::Text("/query".into()));
    }

    #[test]
    fn the_model_reads_the_expanded_question() {
        let record = CommandRecord {
            name: "query".into(),
            args: "errors by service".into(),
            source: CommandSource::Builtin,
            model_text: "errors by service".into(),
            ..CommandRecord::default()
        };

        assert_eq!(
            text_for_model("/query errors by service", Some(&record)),
            "errors by service"
        );
        assert_eq!(text_for_model("\\/etc/hosts", None), "/etc/hosts");
        assert_eq!(text_for_model("plain", None), "plain");
    }
}
