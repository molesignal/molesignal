// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Markdown's inline formatting within one line: `**bold**`, `*italic*`, `~~struck~~`,
//! `` `code` ``, `[links](https://…)` and `<https://…>`, each closed where it opens. What may open
//! depends on where the text stands ([`Within`]): Telegram refuses code inside other formatting
//! and links inside a link or a quote.

use super::{escape, escape_attribute, push_escaped};

/// What may open at this point of a line: code nowhere inside other formatting, links nowhere
/// inside a link or a quote (they are written out there instead).
#[derive(Debug, Clone, Copy)]
pub(super) struct Within {
    pub(super) code: bool,
    pub(super) link: bool,
}

pub(super) const TOP: Within = Within {
    code: true,
    link: true,
};

/// Inside a link or a quote: neither code nor links open.
pub(super) const PLAIN: Within = Within {
    code: false,
    link: false,
};

/// One line of Markdown's inline formatting as Telegram HTML.
pub(crate) fn inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    spans(&chars(text), TOP, &mut out);
    out
}

pub(super) fn spans(chars: &[char], within: Within, out: &mut String) {
    let mut at = 0;

    while at < chars.len() {
        let c = chars[at];

        let next = match c {
            '\\' => escaped(chars, at, out),
            '`' => code_span(chars, at, within, out),
            '*' | '_' | '~' => emphasis(chars, at, within, out),
            '[' => link(chars, at + 1, within, out),
            '!' if chars.get(at + 1) == Some(&'[') => link(chars, at + 2, within, out),
            '<' => autolink(chars, at, within, out),
            _ => None,
        };

        match next {
            Some(next) => at = next,
            None => {
                push_escaped(out, c);
                at += 1;
            }
        }
    }
}

/// `<tag>` and `</tag>` around the spans of `chars`, where no code may open.
pub(super) fn wrapped(out: &mut String, tag: &str, chars: &[char], within: Within) {
    let inner = Within {
        code: false,
        ..within
    };

    out.push('<');
    out.push_str(tag);
    out.push('>');
    spans(chars, inner, out);
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

/// How many `c` follow one another from `at`.
fn run(chars: &[char], at: usize, c: char) -> usize {
    chars[at..].iter().take_while(|&&next| next == c).count()
}

fn escaped(chars: &[char], at: usize, out: &mut String) -> Option<usize> {
    let next = *chars.get(at + 1)?;

    if !next.is_ascii_punctuation() {
        return None;
    }

    push_escaped(out, next);
    Some(at + 2)
}

/// `` `code` ``, closed by a run of as many backticks. An unclosed run is text.
fn code_span(chars: &[char], at: usize, within: Within, out: &mut String) -> Option<usize> {
    let ticks = run(chars, at, '`');
    let start = at + ticks;
    let mut index = start;
    let mut end = None;

    while index < chars.len() {
        if chars[index] != '`' {
            index += 1;
            continue;
        }

        let closing = run(chars, index, '`');

        if closing == ticks {
            end = Some(index);
            break;
        }

        index += closing;
    }

    let Some(end) = end else {
        out.push_str(&"`".repeat(ticks));
        return Some(start);
    };

    let raw = chars[start..end].iter().collect::<String>();

    // One space on each side of the code is padding.
    let code = raw
        .strip_prefix(' ')
        .and_then(|code| code.strip_suffix(' '))
        .filter(|inner| !inner.trim().is_empty())
        .unwrap_or(&raw);

    if within.code && !code.trim().is_empty() {
        out.push_str("<code>");
        out.push_str(&escape(code));
        out.push_str("</code>");
    } else {
        out.push_str(&escape(code));
    }

    Some(end + ticks)
}

/// `**bold**` (or `__bold__`), `*italic*` (or `_italic_`) and `~~struck~~`. A mark opens before a
/// word and closes after one; underscores never open or close inside a word (`snake_case`).
fn emphasis(chars: &[char], at: usize, within: Within, out: &mut String) -> Option<usize> {
    let mark = chars[at];
    let marks = run(chars, at, mark);

    let (width, tag) = match (mark, marks) {
        ('~', 2..) => (2, "s"),
        ('~', _) => return None,
        (_, 2..) => (2, "b"),
        _ => (1, "i"),
    };

    let start = at + width;

    if chars.get(start).is_none_or(|c| c.is_whitespace())
        || (mark == '_' && at > 0 && chars[at - 1].is_alphanumeric())
    {
        return None;
    }

    let mut index = start + 1;

    while index + width <= chars.len() {
        let marked = chars[index..index + width].iter().all(|&c| c == mark);
        let after_word = !chars[index - 1].is_whitespace();
        let whole = width == 2 || (chars[index - 1] != mark && chars.get(index + 1) != Some(&mark));
        let next = chars.get(index + width);
        let in_word = mark == '_' && next.is_some_and(|c| c.is_alphanumeric());

        if marked && after_word && whole && !in_word {
            wrapped(out, tag, &chars[start..index], within);
            return Some(index + width);
        }

        index += 1;
    }

    None
}

/// `[label](address)` (an image's `![label](address)` too), from the first character of the label.
/// Only http(s) and tg:// addresses become links; where no link may open, the address is written
/// after the label.
fn link(chars: &[char], label_start: usize, within: Within, out: &mut String) -> Option<usize> {
    let close = closing(chars, label_start, '[', ']')?;

    if chars.get(close + 1) != Some(&'(') {
        return None;
    }

    let end = closing(chars, close + 2, '(', ')')?;
    let target = chars[close + 2..end].iter().collect::<String>();

    // A title may follow the address, and angle brackets may wrap it.
    let address = target
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_start_matches('<')
        .trim_end_matches('>');

    if !linkable(address) {
        return None;
    }

    let label = &chars[label_start..close];
    let unlabelled = label.iter().all(|c| c.is_whitespace());

    if within.link {
        out.push_str("<a href=\"");
        out.push_str(&escape_attribute(address));
        out.push_str("\">");

        if unlabelled {
            out.push_str(&escape(address));
        } else {
            spans(label, PLAIN, out);
        }

        out.push_str("</a>");
    } else if unlabelled {
        out.push_str(&escape(address));
    } else {
        spans(label, within, out);
        out.push_str(" (");
        out.push_str(&escape(address));
        out.push(')');
    }

    Some(end + 1)
}

/// Where the bracket that `from` sits inside closes, counting nested pairs and skipping escaped
/// characters.
fn closing(chars: &[char], from: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 1usize;
    let mut index = from;

    while index < chars.len() {
        match chars[index] {
            '\\' => index += 1,
            c if c == open => depth += 1,
            c if c == close => {
                depth -= 1;

                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }

        index += 1;
    }

    None
}

/// `<https://…>`: the address as a link.
fn autolink(chars: &[char], at: usize, within: Within, out: &mut String) -> Option<usize> {
    let end = at + 1 + chars[at + 1..].iter().position(|&c| c == '>')?;
    let address = chars[at + 1..end].iter().collect::<String>();

    if !linkable(&address) || address.chars().any(char::is_whitespace) {
        return None;
    }

    if within.link {
        out.push_str("<a href=\"");
        out.push_str(&escape_attribute(&address));
        out.push_str("\">");
        out.push_str(&escape(&address));
        out.push_str("</a>");
    } else {
        out.push_str(&escape(&address));
    }

    Some(end + 1)
}

/// Addresses a Telegram link may open.
pub(crate) fn linkable(address: &str) -> bool {
    ["https://", "http://", "tg://"].iter().any(|scheme| {
        address.len() > scheme.len()
            && address
                .get(..scheme.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(scheme))
    })
}

pub(super) fn chars(text: &str) -> Vec<char> {
    text.chars().collect()
}
