// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Context passed to the model: how to compact long histories and which language to answer in.
//!
//! Token counts are estimated from characters: about 4 ASCII characters per token, one token per
//! other character. When usage exceeds 80% of the context, conversations before the most recent
//! [`COMPACTION_KEEP_TURNS`] turns are compacted into a summary; `/compact` compacts the entire
//! conversation so far. The summary is stored on the session and replaces the compacted messages
//! when subsequently passed to the model.

/// The number of recent turns kept intact during automatic compaction.
pub const COMPACTION_KEEP_TURNS: usize = 2;

/// The instruction that asks the model to write a summary.
pub const COMPACTION_INSTRUCTION: &str = "Summarize the conversation above so that the investigation can continue from the summary alone. Keep what was asked; what was found and with which tools, with the key numbers, identifiers, services, queries and time ranges; the conclusions and the evidence behind them; and what is still open. Leave out greetings and anything that was later corrected. Write at most 800 words.";

/// The prefix used when passing the summary to the model.
const SUMMARY_PREFIX: &str = "Summary of the earlier part of this conversation:";

/// Estimates the token count of a piece of text.
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

/// The history uses 80% of the context.
pub fn compaction_due(tokens: u64, window: u64) -> bool {
    window > 0 && tokens.saturating_mul(5) > window.saturating_mul(4)
}

/// Finds where to start keeping messages intact during compaction: the `keep_turns`-th user
/// question from the end. `roles` contains the role of each message in chronological order; when
/// `keep_turns` is 0, everything is compacted. Returns `None` if there are no earlier messages to
/// compact.
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

/// The summary as presented to the model.
pub fn summary_text(summary: &str) -> String {
    format!("{SUMMARY_PREFIX}\n\n{summary}")
}

/// Tells the model which language to answer in; this is set from the initiator's UI language when
/// the session is created.
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
