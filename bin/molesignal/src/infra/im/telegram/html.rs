// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Telegram's HTML: the few tags Telegram reads, and Markdown (Mole Agent's answers, the bodies of
//! notifications) written with them.
//!
//! Headings and `**bold**` become bold, `*italic*` italic, `~~struck~~` struck, `` `code` `` code,
//! `[text](https://…)` links, fenced code `pre`, quotes `blockquote`, list markers bullets and
//! tables an aligned `pre` block. Code never sits inside other formatting and links never inside
//! quotes (Telegram refuses both), and every tag closes where it opens, so whatever the Markdown,
//! the HTML reads.
//!
//! A message holds at most [`MAX_TEXT`] characters once its tags are read. Longer text is cut
//! between lines into several messages ([`messages`]) or cut short ([`fit`]).

mod inline;

use self::inline::{PLAIN, TOP, Within, chars, spans, wrapped};
pub(crate) use self::inline::{inline, linkable};

/// Telegram's limit on the text of a message once its tags are read, in UTF-16 units as Telegram
/// counts them.
pub(crate) const MAX_TEXT: usize = 4096;

/// Escapes text for Telegram's HTML.
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());

    for c in text.chars() {
        push_escaped(&mut out, c);
    }

    out
}

fn push_escaped(out: &mut String, c: char) {
    match c {
        '&' => out.push_str("&amp;"),
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        c => out.push(c),
    }
}

fn escape_attribute(text: &str) -> String {
    escape(text).replace('"', "&quot;")
}

/// How long `html` reads once its tags are read: UTF-16 units, each entity one.
pub(crate) fn visible_len(html: &str) -> usize {
    let mut len = 0;
    let mut chars = html.chars();

    while let Some(c) = chars.next() {
        match c {
            '<' => skip_past(&mut chars, '>'),
            '&' => {
                skip_past(&mut chars, ';');
                len += 1;
            }
            c => len += c.len_utf16(),
        }
    }

    len
}

/// What `html` reads as: the tags dropped and the entities read.
pub(crate) fn plain(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut chars = html.chars();

    while let Some(c) = chars.next() {
        match c {
            '<' => skip_past(&mut chars, '>'),
            '&' => {
                let entity = chars.by_ref().take_while(|&c| c != ';').collect::<String>();

                out.push(match entity.as_str() {
                    "lt" => '<',
                    "gt" => '>',
                    "quot" => '"',
                    _ => '&',
                });
            }
            c => out.push(c),
        }
    }

    out
}

/// Markdown as the plain text it reads as, for where Telegram shows no formatting (a toast).
pub(crate) fn plain_text(markdown: &str) -> String {
    plain(&messages(markdown, MAX_TEXT).join("\n"))
}

fn skip_past(chars: &mut std::str::Chars<'_>, end: char) {
    for c in chars.by_ref() {
        if c == end {
            break;
        }
    }
}

/// Markdown as Telegram HTML, cut between lines into messages of at most `max` characters.
pub(crate) fn messages(markdown: &str, max: usize) -> Vec<String> {
    let mut messages = Vec::new();
    let mut current = String::new();
    let mut len = 0;

    for piece in pieces(markdown, max) {
        // A message does not start with an empty line.
        if current.is_empty() && piece.html.is_empty() {
            continue;
        }

        if !current.is_empty() && len + 1 + piece.len > max {
            messages.push(current.trim_end().to_string());
            current.clear();
            len = 0;

            if piece.html.is_empty() {
                continue;
            }
        }

        if !current.is_empty() {
            current.push('\n');
            len += 1;
        }

        current.push_str(&piece.html);
        len += piece.len;
    }

    let last = current.trim_end();

    if !last.is_empty() {
        messages.push(last.to_string());
    }

    messages
}

/// Markdown as Telegram HTML in at most `max` characters: what does not fit is left out, and an
/// ellipsis on a line of its own says so.
pub(crate) fn fit(markdown: &str, max: usize) -> String {
    // Too little room for even a character and the ellipsis.
    if max < 3 {
        return String::new();
    }

    let mut whole = messages(markdown, max);

    if whole.len() <= 1 {
        return whole.pop().unwrap_or_default();
    }

    match messages(markdown, max - 2).into_iter().next() {
        Some(first) => format!("{first}\n…"),
        None => String::new(),
    }
}

/// A part of a message that stands alone (a line, or a code block, quote or table, or a share of
/// one too long for a message) with every tag closed, and how long it reads.
struct Piece {
    html: String,
    len: usize,
}

impl Piece {
    fn new(html: String) -> Self {
        let len = visible_len(&html);
        Self { html, len }
    }
}

fn pieces(markdown: &str, max: usize) -> Vec<Piece> {
    let max = max.max(1);
    let lines = markdown.lines().collect::<Vec<_>>();
    let mut pieces = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let trimmed = lines[index].trim_start();

        if let Some(fence) = ["```", "~~~"].into_iter().find(|f| trimmed.starts_with(f)) {
            let end = lines[index + 1..]
                .iter()
                .position(|line| line.trim_start().starts_with(fence))
                .map_or(lines.len(), |at| index + 1 + at);

            let (info, code) = (&trimmed[fence.len()..], &lines[index + 1..end]);
            code_block(&mut pieces, info, code, max);
            index = end + 1;
            continue;
        }

        let rows = lines[index..]
            .iter()
            .take_while(|line| line.trim_start().starts_with('|'))
            .count();

        if rows >= 2 && separator_row(lines[index + 1]) {
            table(&mut pieces, &lines[index..index + rows], max);
            index += rows;
            continue;
        }

        if trimmed.starts_with('>') {
            let quoted = lines[index..]
                .iter()
                .take_while(|line| line.trim_start().starts_with('>'))
                .count();

            quote(&mut pieces, &lines[index..index + quoted], max);
            index += quoted;
            continue;
        }

        // A line longer than a message is cut; its formatting may break where it is cut.
        for part in split_units(lines[index], max) {
            pieces.push(Piece::new(line_html(&part, TOP)));
        }

        index += 1;
    }

    pieces
}

/// A fenced code block, in as many `pre` blocks as it takes.
fn code_block(pieces: &mut Vec<Piece>, info: &str, lines: &[&str], max: usize) {
    if lines.iter().all(|line| line.trim().is_empty()) {
        return;
    }

    let language = info.split_whitespace().next().filter(|language| {
        language
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '_' | '#' | '.'))
    });

    let (open, close) = match language {
        Some(language) => (
            format!("<pre><code class=\"language-{language}\">"),
            "</code></pre>",
        ),
        None => ("<pre>".to_string(), "</pre>"),
    };

    for chunk in lines_within(lines, max) {
        pieces.push(Piece::new(format!("{open}{}{close}", escape(&chunk))));
    }
}

/// Quoted lines, in as many `blockquote` blocks as it takes. Links in a quote are written out.
fn quote(pieces: &mut Vec<Piece>, lines: &[&str], max: usize) {
    let mut groups: Vec<(Vec<String>, usize)> = Vec::new();

    for line in lines {
        let text = line.trim_start().trim_start_matches('>');
        let text = text.strip_prefix(' ').unwrap_or(text);

        for part in split_units(text, max) {
            let html = line_html(&part, PLAIN);
            let len = visible_len(&html);

            match groups.last_mut() {
                Some((group, size)) if *size + 1 + len <= max => {
                    group.push(html);
                    *size += 1 + len;
                }
                _ => groups.push((vec![html], len)),
            }
        }
    }

    for (group, _) in groups {
        let body = group.join("\n");

        if !body.trim().is_empty() {
            pieces.push(Piece::new(format!("<blockquote>{body}</blockquote>")));
        }
    }
}

fn separator_row(line: &str) -> bool {
    let cells = cells(line);

    !cells.is_empty()
        && cells
            .iter()
            .all(|cell| cell.contains('-') && cell.chars().all(|c| matches!(c, '-' | ':' | ' ')))
}

fn cells(line: &str) -> Vec<String> {
    let line = line.trim();
    let line = line.strip_prefix('|').unwrap_or(line);
    let line = line.strip_suffix('|').unwrap_or(line);

    line.split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

/// A Markdown table as an aligned `pre` block: the cells' text padded to their column, a rule
/// under the header.
fn table(pieces: &mut Vec<Piece>, lines: &[&str], max: usize) {
    let rows = lines
        .iter()
        .filter(|line| !separator_row(line))
        .map(|line| {
            cells(line)
                .iter()
                .map(|cell| plain(&inline(cell)))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    let columns = rows.iter().map(Vec::len).max().unwrap_or_default();

    let widths = (0..columns)
        .map(|column| {
            rows.iter()
                .filter_map(|row| row.get(column))
                .map(|cell| cell.chars().count())
                .max()
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();

    let mut text = Vec::with_capacity(rows.len() + 1);

    for (index, row) in rows.iter().enumerate() {
        let line = widths
            .iter()
            .enumerate()
            .map(|(column, &width)| {
                let cell = row.get(column).map_or("", String::as_str);
                format!("{cell:<width$}")
            })
            .collect::<Vec<_>>()
            .join(" | ");

        text.push(line.trim_end().to_string());

        if index == 0 {
            let rule = widths.iter().map(|&width| "-".repeat(width));
            text.push(rule.collect::<Vec<_>>().join("-+-"));
        }
    }

    for chunk in lines_within(&text, max) {
        pieces.push(Piece::new(format!("<pre>{}</pre>", escape(&chunk))));
    }
}

/// Lines of plain text gathered into chunks of at most `max` UTF-16 units each, newlines
/// included; a line longer than that is cut.
fn lines_within<S: AsRef<str>>(lines: &[S], max: usize) -> Vec<String> {
    let mut chunks: Vec<(Vec<String>, usize)> = Vec::new();

    for line in lines
        .iter()
        .flat_map(|line| split_units(line.as_ref(), max))
    {
        let len = units(&line);

        match chunks.last_mut() {
            Some((chunk, size)) if *size + 1 + len <= max => {
                chunk.push(line);
                *size += 1 + len;
            }
            _ => chunks.push((vec![line], len)),
        }
    }

    chunks
        .into_iter()
        .map(|(chunk, _)| chunk.join("\n"))
        .collect()
}

fn units(text: &str) -> usize {
    text.encode_utf16().count()
}

/// `text` cut into parts of at most `max` UTF-16 units, without splitting a character.
fn split_units(text: &str, max: usize) -> Vec<String> {
    let mut parts = Vec::new();
    let mut part = String::new();
    let mut len = 0;

    for c in text.chars() {
        if len + c.len_utf16() > max && !part.is_empty() {
            parts.push(std::mem::take(&mut part));
            len = 0;
        }

        part.push(c);
        len += c.len_utf16();
    }

    parts.push(part);
    parts
}

/// One line outside code blocks and tables: a heading in bold, a list item behind a bullet, a rule
/// as an empty line, anything else with its inline formatting.
fn line_html(line: &str, within: Within) -> String {
    let trimmed = line.trim();

    if trimmed.is_empty() || rule(trimmed) {
        return String::new();
    }

    if let Some(heading) = heading(trimmed) {
        let mut out = String::new();
        wrapped(&mut out, "b", &chars(heading), within);
        return out;
    }

    let start = line.trim_start();
    let indent = &line[..line.len() - start.len()];
    let mut out = String::from(indent);

    match ["- ", "* ", "+ "]
        .iter()
        .find_map(|marker| start.strip_prefix(marker))
    {
        Some(item) => {
            out.push_str("• ");
            spans(&chars(item), within, &mut out);
        }
        None => spans(&chars(start), within, &mut out),
    }

    out
}

/// `# Heading` to `###### Heading`: its text.
fn heading(line: &str) -> Option<&str> {
    let level = line.chars().take_while(|&c| c == '#').count();
    let rest = &line[level..];
    let spaced = rest.is_empty() || rest.starts_with(' ');

    if !(1..=6).contains(&level) || !spaced {
        return None;
    }

    Some(rest.trim().trim_end_matches('#').trim_end()).filter(|text| !text.is_empty())
}

/// A thematic break: three or more of the same `-`, `*` or `_`.
fn rule(line: &str) -> bool {
    let marks = line
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<Vec<_>>();

    marks.len() >= 3
        && ['-', '*', '_']
            .iter()
            .any(|&mark| marks.iter().all(|&c| c == mark))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_escaped_and_formatting_becomes_tags() {
        assert_eq!(
            inline("p99 <2s> & **rising** on `api` see [runbook](https://r.example/a?b=1&c=\"d\")"),
            "p99 &lt;2s&gt; &amp; <b>rising</b> on <code>api</code> see \
             <a href=\"https://r.example/a?b=1&amp;c=&quot;d&quot;\">runbook</a>"
        );
        assert_eq!(
            inline("*italic*, __bold__, ~~gone~~ and snake_case_name"),
            "<i>italic</i>, <b>bold</b>, <s>gone</s> and snake_case_name"
        );
        assert_eq!(inline("2 * 3 * 4 and a ** b"), "2 * 3 * 4 and a ** b");
        assert_eq!(inline(r"\*not italic\* and \`x\`"), "*not italic* and `x`");
        assert_eq!(
            inline(
                "[x](javascript:alert(1)) <https://ms.example/a> ![chart](https://c.example/p.png)"
            ),
            "[x](javascript:alert(1)) <a href=\"https://ms.example/a\">https://ms.example/a</a> \
             <a href=\"https://c.example/p.png\">chart</a>"
        );
    }

    #[test]
    fn code_never_sits_inside_other_formatting_or_links() {
        assert_eq!(
            inline("**run `make` now** and [the `x` docs](https://d.example)"),
            "<b>run make now</b> and <a href=\"https://d.example\">the x docs</a>"
        );
        assert_eq!(
            inline("unclosed `tick and **star"),
            "unclosed `tick and **star"
        );
        assert_eq!(inline("``a ` b``"), "<code>a ` b</code>");
    }

    #[test]
    fn blocks_become_pre_quotes_bullets_and_bold_headings() {
        let html = messages(
            "## Cause & fix\n\n- pool **full**\n  * nested\n---\n> quoted [link](https://q.example)\n> more\n\n```rust\nlet a = b < c;\n```\nafter",
            MAX_TEXT,
        );

        assert_eq!(
            html,
            [
                "<b>Cause &amp; fix</b>\n\n• pool <b>full</b>\n  • nested\n\n\
              <blockquote>quoted link (https://q.example)\nmore</blockquote>\n\n\
              <pre><code class=\"language-rust\">let a = b &lt; c;</code></pre>\nafter"
            ]
        );
    }

    #[test]
    fn a_table_is_an_aligned_block() {
        let html = messages(
            "| Endpoint | p99 |\n|---|---:|\n| /checkout | **2.1 s** |\n| /a | 9 ms |",
            MAX_TEXT,
        );

        assert_eq!(
            html,
            ["<pre>Endpoint  | p99\n----------+------\n/checkout | 2.1 s\n/a        | 9 ms</pre>"]
        );
    }

    #[test]
    fn long_text_is_cut_between_lines_and_every_message_fits() {
        let line = "word ".repeat(30);
        let markdown = (0..100)
            .map(|index| format!("{index} {line}"))
            .collect::<Vec<_>>()
            .join("\n");

        let messages = messages(&markdown, 1_000);

        assert!(messages.len() > 1);
        assert!(messages.iter().all(|message| visible_len(message) <= 1_000));
        assert!(messages[1].starts_with(char::is_numeric));

        let code = format!("```\n{}\n```", "x".repeat(2_500));
        let split = super::messages(&code, 1_000);
        assert_eq!(split.len(), 3);
        assert!(split.iter().all(|message| message.starts_with("<pre>")));
        assert!(split.iter().all(|message| visible_len(message) <= 1_000));
    }

    #[test]
    fn fitting_cuts_short_and_says_so() {
        let markdown = format!("first line\n{}", "y".repeat(200));
        let fitted = fit(&markdown, 50);

        assert!(fitted.ends_with("\n…"));
        assert!(visible_len(&fitted) <= 50);
        assert_eq!(fit("short", 50), "short");
        assert_eq!(fit("", 50), "");
    }

    #[test]
    fn length_counts_what_telegram_shows() {
        assert_eq!(visible_len("<b>a &amp; b</b>"), 5);
        assert_eq!(visible_len("🔴 x"), 4);
        assert_eq!(
            plain("<i>a &lt;b&gt; &amp; &quot;c&quot;</i>"),
            "a <b> & \"c\""
        );
        assert!(linkable("HTTPS://x.example"));
        assert!(!linkable("https://"));
        assert!(!linkable("mailto:a@b.example"));
    }
}
