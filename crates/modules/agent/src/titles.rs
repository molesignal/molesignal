// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Conversation names follow the approach used by ChatGPT and Claude.
//! The conversation page initially uses the first question as a temporary title; the model
//! generates a short name while answering the first question, and the name stays unchanged
//! afterward. The name describes the conversation's topic in the question's language, as a phrase
//! rather than a question, with no quotes, emoji, or ending punctuation. If the question has no
//! topic yet (just a greeting or a test), naming waits for the next question.

/// Naming instructions for the model.
pub const TITLE_INSTRUCTION: &str = "Name this chat for the person's list of chats, the way ChatGPT and Claude name a conversation from its first question. Answer with the name only.
- Say what the chat is about: the subject and what is wanted, as a short noun phrase, not a question or a full sentence.
- Write it in the language the question is written in.
- Languages written with spaces: 2 to 6 words in sentence case (capitalize only the first word and proper nouns). Chinese, Japanese or Korean: at most 15 characters.
- Keep the names of services, hosts, alerts, errors and code exactly as written.
- No quotes, no emoji, no trailing punctuation, and no lead-in such as \"Question about\", \"Help with\" or \"Chat about\".
- If the question has no subject yet (a greeting, a test message), answer with a single hyphen: -

Examples:
Why is checkout-api returning 502 since 10:00? -> checkout-api 502 errors since 10:00
帮我看下 payment 服务最近一小时的错误日志 -> payment 服务近一小时错误日志
hi -> -";

/// The maximum number of characters in the question passed to the model.
pub const QUESTION_CHARS: usize = 2_000;

/// The maximum number of characters in the name; truncates longer model output.
pub const MAX_TITLE_CHARS: usize = 60;

/// The question used for naming: keeps only the beginning if it is too long.
pub fn question_for_title(question: &str) -> String {
    question.trim().chars().take(QUESTION_CHARS).collect()
}

/// Extracts the model-generated name: removes reasoning, prefixes such as "Title:", enclosing
/// quotes, and ending punctuation, and truncates it if too long. If the model returns `-` or
/// nothing, it is not yet time to name the conversation.
pub fn clean_title(text: &str) -> Option<String> {
    let text = match text.rfind("</think>") {
        Some(end) => &text[end + "</think>".len()..],
        None => text,
    };

    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    let line = strip_label(line);

    let line = line
        .trim_matches(|c: char| QUOTES.contains(c) || c.is_whitespace())
        .trim_end_matches(|c: char| TRAILING.contains(c) || c.is_whitespace())
        .trim_matches(|c: char| QUOTES.contains(c) || c.is_whitespace());

    let title = line.split_whitespace().collect::<Vec<_>>().join(" ");

    if title.chars().all(|c| "-—–".contains(c)) {
        return None;
    }

    let title: String = title.chars().take(MAX_TITLE_CHARS).collect();
    Some(title.trim_end().to_string())
}

/// Quotes, brackets, and Markdown markers enclosing the name.
const QUOTES: &str = "\"'`*#“”‘’「」『』《》【】";

/// Unwanted punctuation at the end of the name.
const TRAILING: &str = ".。!！?？,，:：;；、…";

/// Removes the "Title:" / `标题：` prefix the model occasionally adds.
fn strip_label(line: &str) -> &str {
    for label in ["title:", "title：", "标题：", "标题:", "name:", "名字："] {
        let starts = line
            .get(..label.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(label));

        if starts {
            return line[label.len()..].trim();
        }
    }

    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_name_is_kept() {
        assert_eq!(
            clean_title("checkout-api 502 errors since 10:00").as_deref(),
            Some("checkout-api 502 errors since 10:00")
        );
        assert_eq!(
            clean_title("payment 服务近一小时错误日志").as_deref(),
            Some("payment 服务近一小时错误日志")
        );
    }

    #[test]
    fn quotes_labels_and_trailing_marks_are_dropped() {
        assert_eq!(
            clean_title("\"Slow checkout queries.\"").as_deref(),
            Some("Slow checkout queries")
        );
        assert_eq!(
            clean_title("Title: Kafka consumer lag").as_deref(),
            Some("Kafka consumer lag")
        );
        assert_eq!(
            clean_title("标题：「订单服务超时排查」。").as_deref(),
            Some("订单服务超时排查")
        );
        assert_eq!(
            clean_title("**Disk usage on db-01**").as_deref(),
            Some("Disk usage on db-01")
        );
    }

    #[test]
    fn only_the_first_line_after_any_thinking_counts() {
        assert_eq!(
            clean_title("<think>the user asks about nginx</think>\n\nNginx 499 spikes\nmore")
                .as_deref(),
            Some("Nginx 499 spikes")
        );
        assert_eq!(
            clean_title("  \n  Memory  leak   in  worker \n").as_deref(),
            Some("Memory leak in worker")
        );
    }

    #[test]
    fn no_subject_means_no_name_yet() {
        assert_eq!(clean_title("-"), None);
        assert_eq!(clean_title(" — "), None);
        assert_eq!(clean_title(""), None);
        assert_eq!(clean_title("\"\""), None);
    }

    #[test]
    fn a_long_name_is_cut() {
        let long = "word ".repeat(40);
        let title = clean_title(&long).unwrap();

        assert!(title.chars().count() <= MAX_TITLE_CHARS);
        assert!(!title.ends_with(' '));
    }

    #[test]
    fn a_long_question_is_cut_for_the_model() {
        let question = "问".repeat(QUESTION_CHARS + 10);

        assert_eq!(
            question_for_title(&question).chars().count(),
            QUESTION_CHARS
        );
    }
}
