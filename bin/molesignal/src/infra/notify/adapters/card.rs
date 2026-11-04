// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 即时通讯模板里的卡片（[`NotifyCard`]）：飞书、钉钉、企业微信、Slack、Teams、Google Chat 的 App 与
//! 群机器人各按自己的卡片格式发出，这里是它们共用的部分：标题、颜色与把正文排进 HTML 的写法。

use crate::domain::notify::{
    connector::NotifyMessage,
    template::{NotifyCard, NotifyCardColor},
};

/// 卡片标题的颜色，按严重度的那种已经按这条通知换成了具体的一种。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CardColor {
    Blue,
    Green,
    Yellow,
    Orange,
    Red,
    Grey,
}

impl CardColor {
    /// 网页上的颜色：Slack 的色条、钉钉 Markdown 的字色都用它。
    pub(crate) const fn hex(self) -> &'static str {
        match self {
            Self::Blue => "#1A73E8",
            Self::Green => "#1E8E3E",
            Self::Yellow => "#F9AB00",
            Self::Orange => "#E8710A",
            Self::Red => "#D93025",
            Self::Grey => "#80868B",
        }
    }
}

/// 告警恢复或关闭了是绿色，其余按严重度：严重是红、错误是橙、警告是黄，其他是蓝。
pub(crate) fn severity_color(message: &NotifyMessage) -> CardColor {
    let metadata = &message.metadata;

    if matches!(
        metadata.get("status").map(String::as_str),
        Some("resolved" | "closed")
    ) {
        return CardColor::Green;
    }

    match metadata.get("severity").map(String::as_str) {
        Some("critical") => CardColor::Red,
        Some("error") => CardColor::Orange,
        Some("warning") => CardColor::Yellow,
        _ => CardColor::Blue,
    }
}

/// 卡片的颜色：选了一种就是它，按严重度的按这条通知算。
pub(crate) fn card_color(card: &NotifyCard, message: &NotifyMessage) -> CardColor {
    match card.color {
        NotifyCardColor::Severity => severity_color(message),
        NotifyCardColor::Blue => CardColor::Blue,
        NotifyCardColor::Green => CardColor::Green,
        NotifyCardColor::Yellow => CardColor::Yellow,
        NotifyCardColor::Orange => CardColor::Orange,
        NotifyCardColor::Red => CardColor::Red,
        NotifyCardColor::Grey => CardColor::Grey,
    }
}

/// 卡片的标题：卡片里没写就是通知的标题。
pub(crate) fn card_title<'a>(card: &'a NotifyCard, message: &'a NotifyMessage) -> &'a str {
    match card.title.trim() {
        "" => message.title.trim(),
        title => title,
    }
}

/// Markdown 排进只认少数 HTML 标签的卡片文字（Google Chat）：转义后，标题行与 `**` 加粗，`[文字](链接)`
/// 换成链接，行内代码去掉反引号，换行用 `<br>`。
pub(crate) fn markdown_html(markdown: &str) -> String {
    markdown
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            let heading = trimmed.trim_start_matches('#');
            let escaped = escape_html(line);

            if trimmed.starts_with('#') && heading.starts_with(' ') {
                format!("<b>{}</b>", escape_html(heading.trim()))
            } else {
                links_html(&bold_html(&escaped.replace('`', "")))
            }
        })
        .collect::<Vec<_>>()
        .join("<br>")
}

pub(crate) fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// 成对的 `**` 换成 `<b>`；落单的照原样。
fn bold_html(line: &str) -> String {
    let parts = line.split("**").collect::<Vec<_>>();

    if parts.len() < 3 {
        return line.to_string();
    }

    let mut out = String::with_capacity(line.len());

    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            let closing = index % 2 == 0;
            let paired = closing || index + 1 < parts.len();

            out.push_str(match (paired, closing) {
                (false, _) => "**",
                (true, false) => "<b>",
                (true, true) => "</b>",
            });
        }

        out.push_str(part);
    }

    out
}

/// `[文字](链接)` 换成 `<a href="链接">文字</a>`，只认 http(s) 链接。
fn links_html(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;

    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find("](").map(|at| open + at) else {
            break;
        };

        let Some(end) = rest[close + 2..].find(')').map(|at| close + 2 + at) else {
            break;
        };

        let label = &rest[open + 1..close];
        let url = &rest[close + 2..end];

        out.push_str(&rest[..open]);

        if url.starts_with("https://") || url.starts_with("http://") {
            out.push_str(&format!("<a href=\"{url}\">{label}</a>"));
        } else {
            out.push_str(&rest[open..=end]);
        }

        rest = &rest[end + 1..];
    }

    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn message(severity: &str, status: &str) -> NotifyMessage {
        NotifyMessage {
            title: "Disk full".into(),
            text: String::new(),
            markdown: None,
            html: None,
            metadata: BTreeMap::from([
                ("severity".to_string(), severity.to_string()),
                ("status".to_string(), status.to_string()),
            ]),
            provider_templates: BTreeMap::new(),
            event: None,
            card: None,
        }
    }

    #[test]
    fn the_colour_follows_the_severity_until_the_alert_is_over() {
        let card = NotifyCard::default();

        assert_eq!(
            card_color(&card, &message("critical", "open")),
            CardColor::Red
        );
        assert_eq!(
            card_color(&card, &message("warning", "open")),
            CardColor::Yellow
        );
        assert_eq!(
            card_color(&card, &message("critical", "resolved")),
            CardColor::Green
        );

        let fixed = NotifyCard {
            color: NotifyCardColor::Grey,
            ..NotifyCard::default()
        };
        assert_eq!(
            card_color(&fixed, &message("critical", "open")),
            CardColor::Grey
        );
        assert_eq!(card_title(&fixed, &message("info", "open")), "Disk full");
    }

    #[test]
    fn markdown_becomes_the_few_tags_a_card_reads() {
        assert_eq!(
            markdown_html(
                "## Disk\n**db-1** is at `97%` <hot>\nSee [runbook](https://r.example.com) or [x](javascript:alert)"
            ),
            "<b>Disk</b><br><b>db-1</b> is at 97% &lt;hot&gt;<br>See <a href=\"https://r.example.com\">runbook</a> or [x](javascript:alert)"
        );
        assert_eq!(markdown_html("a ** b"), "a ** b");
    }
}
