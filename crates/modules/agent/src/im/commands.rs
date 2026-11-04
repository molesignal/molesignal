// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! IM 里自己的命令：`/help`、`/new`、`/scope`、`/feedback`。
//!
//! 别的斜杠命令（`/rca`、`/insight`、Skill……）不在这里，照常交给 Mole Agent。

/// `/scope` 要换成什么会话。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeTarget {
    Personal,
    /// 团队的名称或 ID。
    Team(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImCommand {
    Help,
    /// 这个话题从新会话开始。
    New,
    /// 没带参数时说现在是什么会话。
    Scope(Option<ScopeTarget>),
    /// 评价这个话题里最近的一个回答。
    Feedback {
        helpful: bool,
        comment: String,
    },
}

/// 认不出的用法：回复用法说明。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Usage {
    Feedback,
}

const PERSONAL: [&str; 4] = ["me", "personal", "个人", "我"];
const HELPFUL: [&str; 6] = ["up", "good", "+1", "👍", "有帮助", "好"];
const UNHELPFUL: [&str; 6] = ["down", "bad", "-1", "👎", "没帮助", "差"];

/// 是 IM 命令时解析它；不是时返回 `None`，交给 Mole Agent。
pub fn parse(text: &str) -> Option<Result<ImCommand, Usage>> {
    let text = text.trim();
    let rest = text.strip_prefix('/')?;

    let (name, argument) = match rest.split_once(char::is_whitespace) {
        Some((name, argument)) => (name, argument.trim()),
        None => (rest, ""),
    };

    let command = match name.to_ascii_lowercase().as_str() {
        "help" => Ok(ImCommand::Help),
        "new" => Ok(ImCommand::New),
        "scope" if argument.is_empty() => Ok(ImCommand::Scope(None)),
        "scope" if PERSONAL.contains(&argument.to_lowercase().as_str()) => {
            Ok(ImCommand::Scope(Some(ScopeTarget::Personal)))
        }
        "scope" => Ok(ImCommand::Scope(Some(ScopeTarget::Team(
            argument.to_string(),
        )))),
        "feedback" => feedback(argument),
        _ => return None,
    };

    Some(command)
}

fn feedback(argument: &str) -> Result<ImCommand, Usage> {
    let (rating, comment) = match argument.split_once(char::is_whitespace) {
        Some((rating, comment)) => (rating, comment.trim()),
        None => (argument, ""),
    };

    let rating = rating.to_lowercase();

    let helpful = if HELPFUL.contains(&rating.as_str()) {
        true
    } else if UNHELPFUL.contains(&rating.as_str()) {
        false
    } else {
        return Err(Usage::Feedback);
    };

    Ok(ImCommand::Feedback {
        helpful,
        comment: comment.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn im_commands_are_read_and_the_rest_goes_to_the_agent() {
        assert_eq!(parse("/help"), Some(Ok(ImCommand::Help)));
        assert_eq!(parse("  /NEW "), Some(Ok(ImCommand::New)));
        assert_eq!(parse("/scope"), Some(Ok(ImCommand::Scope(None))));
        assert_eq!(
            parse("/scope 个人"),
            Some(Ok(ImCommand::Scope(Some(ScopeTarget::Personal))))
        );
        assert_eq!(
            parse("/scope Payments SRE"),
            Some(Ok(ImCommand::Scope(Some(ScopeTarget::Team(
                "Payments SRE".into()
            )))))
        );
        assert_eq!(parse("/rca INC-12"), None);
        assert_eq!(parse("why is checkout slow?"), None);
    }

    #[test]
    fn feedback_takes_a_rating_and_an_optional_comment() {
        assert_eq!(
            parse("/feedback 👎 wrong service"),
            Some(Ok(ImCommand::Feedback {
                helpful: false,
                comment: "wrong service".into()
            }))
        );
        assert_eq!(
            parse("/feedback up"),
            Some(Ok(ImCommand::Feedback {
                helpful: true,
                comment: String::new()
            }))
        );
        assert_eq!(parse("/feedback maybe"), Some(Err(Usage::Feedback)));
        assert_eq!(parse("/feedback"), Some(Err(Usage::Feedback)));
    }
}
