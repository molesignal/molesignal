// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 对话的名字，照 ChatGPT 与 Claude 的做法：对话页先拿第一个问题当临时标题，回答第一个问题时
//! 由模型起一个短名，之后不再变。名字说这场对话是关于什么的，用问题的语言，是短语不是问句，
//! 不带引号、表情和句末标点；问题还没有主题（只是打招呼、试一下）时不起，等下一个问题。

/// 交给模型的起名要求。
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

/// 交给模型的问题最多这么多字符。
pub const QUESTION_CHARS: usize = 2_000;

/// 名字最多这么多字符；模型写长了截断。
pub const MAX_TITLE_CHARS: usize = 60;

/// 起名用的问题：太长时只留开头。
pub fn question_for_title(question: &str) -> String {
    question.trim().chars().take(QUESTION_CHARS).collect()
}

/// 读出模型起的名字：去掉思考过程、"Title:" 一类的前缀、包着的引号与句末标点，太长截断。
/// 模型回 `-` 或什么都没有时是还不该起名。
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

/// 包在名字两边的引号、括号与 Markdown 记号。
const QUOTES: &str = "\"'`*#“”‘’「」『』《》【】";

/// 名字末尾不要的标点。
const TRAILING: &str = ".。!！?？,，:：;；、…";

/// 去掉模型偶尔写在前面的"Title:" / "标题："。
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
