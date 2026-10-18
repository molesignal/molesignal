// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 交给模型的上下文：历史太长时怎么压缩，回答用什么语言。
//!
//! token 数按字符估算：ASCII 约 4 个字符一个，其余每个字符一个。超过上下文的 80% 时，把
//! 最近 [`COMPACTION_KEEP_TURNS`] 轮之前的对话压缩成摘要；`/compact` 把到现在为止的全部
//! 对话压缩。摘要存在会话上，之后交给模型时代替被压缩的那些消息。

/// 自动压缩时原样保留最近几轮。
pub const COMPACTION_KEEP_TURNS: usize = 2;

/// 让模型写摘要的指令。
pub const COMPACTION_INSTRUCTION: &str = "Summarize the conversation above so that the investigation can continue from the summary alone. Keep what was asked; what was found and with which tools, with the key numbers, identifiers, services, queries and time ranges; the conclusions and the evidence behind them; and what is still open. Leave out greetings and anything that was later corrected. Write at most 800 words.";

/// 摘要交给模型时的开头。
const SUMMARY_PREFIX: &str = "Summary of the earlier part of this conversation:";

/// 估算一段文字的 token 数。
pub fn estimate_tokens(text: &str) -> u64 {
    let (ascii, other) = text
        .chars()
        .fold((0u64, 0u64), |(ascii, other), character| {
            if character.is_ascii() {
                (ascii + 1, other)
            } else {
                (ascii, other + 1)
            }
        });

    ascii.div_ceil(4) + other
}

/// 历史用到了上下文的 80%。
pub fn compaction_due(tokens: u64, window: u64) -> bool {
    window > 0 && tokens.saturating_mul(5) > window.saturating_mul(4)
}

/// 压缩时从哪条消息起原样保留：倒数第 `keep_turns` 个用户问题。`roles` 是各条消息的角色，
/// 按时间先后；`keep_turns` 为 0 时全部压缩。前面没有可压缩的消息时返回 `None`。
pub fn compaction_split(roles: &[&str], keep_turns: usize) -> Option<usize> {
    if keep_turns == 0 {
        return (!roles.is_empty()).then_some(roles.len());
    }

    let questions = roles
        .iter()
        .enumerate()
        .filter(|(_, role)| **role == "user")
        .map(|(position, _)| position)
        .collect::<Vec<_>>();

    if questions.len() <= keep_turns {
        return None;
    }

    let split = questions[questions.len() - keep_turns];
    (split > 0).then_some(split)
}

/// 摘要交给模型时的样子。
pub fn summary_text(summary: &str) -> String {
    format!("{SUMMARY_PREFIX}\n\n{summary}")
}

/// 告诉模型用哪种语言回答；语言在会话创建时按发起人的界面语言定下。
pub fn language_instruction(language: &str) -> Option<String> {
    let name = match language.trim().to_ascii_lowercase().as_str() {
        "" => return None,
        "zh-cn" | "zh" | "zh-hans" => "Simplified Chinese",
        "en-us" | "en" => "English",
        other => {
            return Some(format!(
                "Write every answer in the language with the code `{other}`."
            ));
        }
    };

    Some(format!(
        "Write every answer in {name}, whatever language the data and tool results are in. Keep identifiers, field names and quoted log lines as they are."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_estimated_by_script() {
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
        assert_eq!(estimate_tokens("日志错误"), 4);
        assert!(compaction_due(81, 100));
        assert!(!compaction_due(80, 100));
        assert!(!compaction_due(1_000, 0));
    }

    #[test]
    fn the_last_turns_are_kept_as_they_are() {
        let roles = [
            "user",
            "assistant",
            "user",
            "assistant",
            "user",
            "assistant",
            "user",
        ];

        assert_eq!(compaction_split(&roles, 2), Some(4));
        assert_eq!(compaction_split(&roles, 4), None);
        assert_eq!(compaction_split(&roles, 0), Some(7));
        assert_eq!(compaction_split(&[], 0), None);
        assert_eq!(compaction_split(&["user", "assistant"], 1), None);
    }

    #[test]
    fn the_language_is_named_for_the_model() {
        assert!(
            language_instruction("zh-CN")
                .unwrap()
                .contains("Simplified Chinese")
        );
        assert!(language_instruction("en-us").unwrap().contains("English"));
        assert!(language_instruction("ja-jp").unwrap().contains("`ja-jp`"));
        assert!(language_instruction(" ").is_none());
        assert!(summary_text("x").ends_with("\n\nx"));
    }
}
