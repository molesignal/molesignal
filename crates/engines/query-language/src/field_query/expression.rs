// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A field query read as the alternatives and conditions it joins.
//!
//! `AND` binds tighter than `OR` and brackets group, both words in any case:
//! `a OR b AND c` is `a OR (b AND c)`. A word inside quotes, or inside the
//! brackets of a value (`MATCH(message, 'a or b')`, `x in ('a', 'b')`), is no
//! keyword. What a part between the keywords says is for each reader to read.
//! Kept in step with `web/src/shell/query/fieldExpression.ts`.

/// A field query as the groups and parts it is made of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldExpression {
    /// Parts that all hold.
    And(Vec<FieldExpression>),
    /// Parts one of which holds.
    Or(Vec<FieldExpression>),
    /// What stands between the keywords, as written.
    Part(String),
}

/// The most groups a query nests in brackets. Deeper text stays one part,
/// which the readers refuse, so the reading never recurses without bound.
pub const MAX_NESTING: usize = 32;

#[derive(Clone, Copy)]
enum Keyword {
    And,
    Or,
}

impl Keyword {
    fn word(self) -> &'static str {
        match self {
            Self::And => "AND",
            Self::Or => "OR",
        }
    }
}

/// The statement read as AND and OR of its parts; `None` for one that says nothing.
pub fn parse_field_expression(statement: &str) -> Option<FieldExpression> {
    read_expression(statement, 0)
}

/// Whether the text joins alternatives with an `OR` that no bracket holds.
pub fn has_top_level_or(text: &str) -> bool {
    split_at(text, Keyword::Or).len() > 1
}

/// What a part that is one quoted string says, `"connection reset"` as
/// `connection reset`: text found as a phrase, keywords and all. `None` for a
/// part that is not one quoted string.
pub fn quoted_text(part: &str) -> Option<String> {
    let characters: Vec<char> = part.chars().collect();
    let quote = *characters.first()?;

    if (quote != '\'' && quote != '"') || characters.len() < 2 {
        return None;
    }

    let mut value = String::new();
    let mut index = 1;

    while index < characters.len() {
        let character = characters[index];

        if character == '\\' && index < characters.len() - 1 {
            value.push(match characters[index + 1] {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                other => other,
            });
            index += 2;
            continue;
        }

        if character == quote {
            return (index == characters.len() - 1).then_some(value);
        }

        value.push(character);
        index += 1;
    }

    None
}

/// Walks `characters` outside quotes, keeping the bracket depth, and calls
/// `visit(index, depth)` at every character that is neither quoted nor a
/// bracket; `visit` answers how many characters it consumed past this one.
fn scan(characters: &[char], mut visit: impl FnMut(usize, usize) -> usize) {
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut index = 0;

    while index < characters.len() {
        let character = characters[index];

        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if let Some(open) = quote {
            if character == open {
                quote = None;
            }
        } else if character == '\'' || character == '"' {
            quote = Some(character);
        } else if character == '(' {
            depth += 1;
        } else if character == ')' {
            depth = depth.saturating_sub(1);
        } else {
            index += visit(index, depth);
        }

        index += 1;
    }
}

/// The pieces of `text` between the keyword where it stands outside quotes and
/// brackets, each trimmed; empty pieces are left out.
fn split_at(text: &str, keyword: Keyword) -> Vec<String> {
    let characters: Vec<char> = text.chars().collect();
    let length = keyword.word().len();
    let mut pieces = Vec::new();
    let mut start = 0;

    scan(&characters, |index, depth| {
        if depth > 0 || !starts_keyword(&characters, index, keyword) {
            return 0;
        }

        pieces.push(characters[start..index].iter().collect::<String>());
        start = index + length;
        length - 1
    });

    pieces.push(characters[start.min(characters.len())..].iter().collect());

    pieces
        .into_iter()
        .map(|piece| piece.trim().to_string())
        .filter(|piece| !piece.is_empty())
        .collect()
}

/// Whether `keyword` stands at `index` as a word of its own: spaces or brackets around it.
fn starts_keyword(characters: &[char], index: usize, keyword: Keyword) -> bool {
    let word: Vec<char> = keyword.word().chars().collect();
    let end = index + word.len();

    if end > characters.len()
        || !characters[index..end]
            .iter()
            .zip(&word)
            .all(|(character, letter)| character.to_ascii_uppercase() == *letter)
    {
        return false;
    }

    let before = index.checked_sub(1).map_or(' ', |at| characters[at]);
    let after = characters.get(end).copied().unwrap_or(' ');

    (before.is_whitespace() || before == ')') && (after.is_whitespace() || after == '(')
}

/// The inside of a part that is one group in brackets, `(a OR b)`; `None` for any other part.
fn bracketed(part: &str) -> Option<String> {
    let characters: Vec<char> = part.chars().collect();

    if characters.first() != Some(&'(') || characters.last() != Some(&')') {
        return None;
    }

    let mut depth = 0i64;
    let mut quote: Option<char> = None;
    let mut escaped = false;

    for (index, character) in characters.iter().copied().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }

        if character == '\\' {
            escaped = true;
            continue;
        }

        if let Some(open) = quote {
            if character == open {
                quote = None;
            }

            continue;
        }

        if character == '\'' || character == '"' {
            quote = Some(character);
            continue;
        }

        if character == '(' {
            depth += 1;
        }

        if character != ')' {
            continue;
        }

        depth -= 1;

        // The first bracket closes before the end: `(a) OR (b)` is two groups.
        if depth == 0 {
            return (index == characters.len() - 1)
                .then(|| characters[1..characters.len() - 1].iter().collect());
        }
    }

    None
}

/// A group of `kind` over `items`, a group of the same kind among them taken apart.
fn group(kind: Keyword, items: Vec<FieldExpression>) -> FieldExpression {
    if items.len() == 1 {
        return items.into_iter().next().expect("one item");
    }

    let mut flat = Vec::with_capacity(items.len());

    for item in items {
        match (kind, item) {
            (Keyword::And, FieldExpression::And(inner))
            | (Keyword::Or, FieldExpression::Or(inner)) => flat.extend(inner),
            (_, other) => flat.push(other),
        }
    }

    match kind {
        Keyword::And => FieldExpression::And(flat),
        Keyword::Or => FieldExpression::Or(flat),
    }
}

fn read_part(part: String, depth: usize) -> FieldExpression {
    if depth >= MAX_NESTING {
        return FieldExpression::Part(part);
    }

    let Some(inside) = bracketed(&part) else {
        return FieldExpression::Part(part);
    };

    read_expression(&inside, depth + 1).unwrap_or(FieldExpression::Part(part))
}

fn read_expression(text: &str, depth: usize) -> Option<FieldExpression> {
    let alternatives: Vec<FieldExpression> = split_at(text, Keyword::Or)
        .into_iter()
        .filter_map(|alternative| {
            let parts: Vec<FieldExpression> = split_at(&alternative, Keyword::And)
                .into_iter()
                .map(|part| read_part(part, depth))
                .collect();

            (!parts.is_empty()).then(|| group(Keyword::And, parts))
        })
        .collect();

    (!alternatives.is_empty()).then(|| group(Keyword::Or, alternatives))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(text: &str) -> FieldExpression {
        FieldExpression::Part(text.into())
    }

    #[test]
    fn and_binds_tighter_than_or() {
        assert_eq!(
            parse_field_expression("a OR b AND c"),
            Some(FieldExpression::Or(vec![
                part("a"),
                FieldExpression::And(vec![part("b"), part("c")]),
            ]))
        );
    }

    #[test]
    fn keywords_inside_quotes_or_brackets_are_text() {
        assert_eq!(
            parse_field_expression("MATCH(message, 'a or b') and x in ('c', 'd')"),
            Some(FieldExpression::And(vec![
                part("MATCH(message, 'a or b')"),
                part("x in ('c', 'd')"),
            ]))
        );
        assert_eq!(
            parse_field_expression("ORDER = 'x'"),
            Some(part("ORDER = 'x'"))
        );
    }

    #[test]
    fn a_bracketed_group_is_read_and_flattened() {
        assert_eq!(
            parse_field_expression("(a AND b) AND (c OR d)"),
            Some(FieldExpression::And(vec![
                part("a"),
                part("b"),
                FieldExpression::Or(vec![part("c"), part("d")]),
            ]))
        );
        assert_eq!(parse_field_expression("  "), None);
    }

    #[test]
    fn brackets_nested_past_the_limit_stay_one_part() {
        let deep = format!(
            "{}a = 1{}",
            "(".repeat(MAX_NESTING + 5),
            ")".repeat(MAX_NESTING + 5)
        );

        assert!(matches!(
            parse_field_expression(&deep),
            Some(FieldExpression::Part(text)) if text.starts_with('(')
        ));
    }

    #[test]
    fn a_quoted_string_is_a_phrase() {
        assert_eq!(
            quoted_text(r#""connection reset""#).as_deref(),
            Some("connection reset")
        );
        assert_eq!(quoted_text(r"'it\'s'").as_deref(), Some("it's"));
        assert_eq!(quoted_text("'open"), None);
        assert_eq!(quoted_text("plain"), None);
    }
}
