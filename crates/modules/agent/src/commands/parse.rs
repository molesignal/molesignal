// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 输入原文 → 普通文字或命令。规则与前端 `web/src/routes/agent/chat/composer/commands.ts`
//! 一致：
//!
//! - 去掉开头的空白后，`/` 加一个字母开头、到第一个空白为止的一段只含字母、数字、`_`、`-`、
//!   `:` 时是命令；名称不分大小写，其后去掉首尾空白是参数。
//! - 名称后可以跟 `@团队`（到空白为止），用来指定某个团队的 Skill。
//! - 这一段里还有别的字符（例如 `/var/log/app.log`）时是普通文字。
//! - 以 `\/` 开头是转义：去掉反斜杠，按普通文字处理。

use super::CommandRecord;

/// 一次命令调用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// 小写的命令名。
    pub name: String,
    /// `/名称@团队` 里的团队写法。
    pub team: Option<String>,
    pub args: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// 普通文字；转义的反斜杠已经去掉。
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

/// 一条用户消息交给模型的文字：命令用记下的展开问题，转义去掉反斜杠，其余照原文。
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
