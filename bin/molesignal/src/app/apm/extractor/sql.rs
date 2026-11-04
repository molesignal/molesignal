// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! SQL statements as templates: every literal (strings, numbers, hex and
//! dollar-quoted text) a `?`, comments gone, a list of values one `?`, blanks
//! one space. What is kept is the statement's shape, the tables and columns
//! it names, never a value: text in double quotes is kept only for the
//! databases where it names something, and is a value everywhere else.

/// The longest template kept, in bytes.
const MAX_TEMPLATE_BYTES: usize = 512;
/// The longest statement looked at, in bytes.
const MAX_STATEMENT_BYTES: usize = 16 * 1024;

/// What a statement may start with to be read as SQL.
const LEADING_KEYWORDS: [&str; 24] = [
    "select", "insert", "update", "delete", "with", "merge", "replace", "upsert", "call", "exec",
    "execute", "create", "alter", "drop", "truncate", "show", "set", "values", "copy", "explain",
    "begin", "commit", "rollback", "lock",
];

/// The databases (`db.system.name`) where double quotes name a table or a column.
const QUOTED_NAME_SYSTEMS: [&str; 22] = [
    "postgresql",
    "oracle",
    "db2",
    "sqlite",
    "mssql",
    "cockroachdb",
    "redshift",
    "snowflake",
    "h2",
    "hsqldb",
    "derby",
    "clickhouse",
    "trino",
    "presto",
    "duckdb",
    "teradata",
    "vertica",
    "hanadb",
    "informix",
    "firebird",
    "opengauss",
    "kingbase",
];

/// The template of `statement` sent to `system`; `None` when it does not read as SQL.
pub(super) fn sql_template(statement: &str, system: &str) -> Option<String> {
    if statement.len() > MAX_STATEMENT_BYTES {
        return None;
    }

    let quoted_names = QUOTED_NAME_SYSTEMS.contains(&system.to_ascii_lowercase().as_str());
    let masked = mask_literals(statement, quoted_names);
    let collapsed = collapse_lists(&masked);
    let template = collapsed.trim();
    let first = template
        .split(|char: char| !char.is_ascii_alphabetic())
        .find(|word| !word.is_empty())?
        .to_ascii_lowercase();

    if !LEADING_KEYWORDS.contains(&first.as_str()) {
        return None;
    }

    Some(bounded(template))
}

/// Literals as `?`, comments dropped, blanks one space; text in double
/// quotes kept when it names something (`quoted_names`).
fn mask_literals(statement: &str, quoted_names: bool) -> String {
    let chars = statement.chars().collect::<Vec<_>>();
    let mut out = String::with_capacity(statement.len());
    let mut at = 0;

    while at < chars.len() {
        let char = chars[at];
        let next = chars.get(at + 1).copied();
        let after_word = out.chars().last().is_some_and(is_word);

        match char {
            '-' if next == Some('-') => at = skip_until(&chars, at, "\n"),
            '/' if next == Some('*') => at = skip_until(&chars, at + 2, "*/"),
            '\'' | '"' if char == '\'' || !quoted_names => {
                // A prefix such as E'…', N'…' or X'…' belongs to the literal.
                if out.ends_with(|last: char| "eEnNxXbBuU".contains(last))
                    && !out[..out.len() - 1].ends_with(is_word)
                {
                    out.pop();
                }

                out.push('?');
                at = skip_quoted(&chars, at);
            }
            '$' if next.is_some_and(|next| next == '$' || next.is_ascii_alphabetic()) => {
                match dollar_quote(&chars, at) {
                    Some(end) => {
                        out.push('?');
                        at = end;
                    }
                    None => {
                        out.push(char);
                        at += 1;
                    }
                }
            }
            '"' | '`' => {
                let end = skip_quoted(&chars, at);
                out.extend(&chars[at..end]);
                at = end;
            }
            _ if char.is_ascii_digit() && !after_word => {
                at = skip_number(&chars, at);
                out.push('?');
            }
            _ if char.is_whitespace() => {
                if !out.ends_with(' ') && !out.is_empty() {
                    out.push(' ');
                }

                at += 1;
            }
            _ => {
                out.push(char);
                at += 1;
            }
        }
    }

    out
}

/// `(?, ?, ?)` as `(?)`, and `(?), (?)` as `(?)`: lists of values are one value.
fn collapse_lists(template: &str) -> String {
    let mut out = template.to_owned();

    for (from, to) in [
        ("?, ?", "?"),
        ("?,?", "?"),
        ("(?), (?)", "(?)"),
        ("(?),(?)", "(?)"),
    ] {
        while out.contains(from) {
            out = out.replace(from, to);
        }
    }

    out
}

fn is_word(char: char) -> bool {
    char.is_alphanumeric() || char == '_' || char == '$' || char == '.'
}

/// Past `end`, from `from` on; the end of the text when it never comes.
fn skip_until(chars: &[char], from: usize, end: &str) -> usize {
    let end = end.chars().collect::<Vec<_>>();
    let mut at = from;

    while at < chars.len() {
        if chars[at..].starts_with(&end) {
            return at + end.len();
        }

        at += 1;
    }

    chars.len()
}

/// Past the quote that closes the one at `at`, a doubled quote or a
/// backslash keeping it open.
fn skip_quoted(chars: &[char], at: usize) -> usize {
    let quote = chars[at];
    let mut index = at + 1;

    while index < chars.len() {
        match chars[index] {
            '\\' => index += 2,
            char if char == quote && chars.get(index + 1) == Some(&quote) => index += 2,
            char if char == quote => return index + 1,
            _ => index += 1,
        }
    }

    chars.len()
}

/// Past a number at `at`: digits, a decimal part, an exponent, or hex.
fn skip_number(chars: &[char], at: usize) -> usize {
    let mut index = at;

    if chars[index] == '0'
        && chars
            .get(index + 1)
            .is_some_and(|char| matches!(char, 'x' | 'X'))
    {
        index += 2;

        while chars.get(index).is_some_and(char::is_ascii_hexdigit) {
            index += 1;
        }

        return index;
    }

    while let Some(char) = chars.get(index) {
        let exponent_sign =
            matches!(char, '+' | '-') && matches!(chars.get(index - 1), Some('e' | 'E'));

        if char.is_ascii_digit() || *char == '.' || matches!(char, 'e' | 'E') || exponent_sign {
            index += 1;
        } else {
            break;
        }
    }

    index
}

/// Past a dollar-quoted string (`$$…$$`, `$tag$…$tag$`) starting at `at`.
fn dollar_quote(chars: &[char], at: usize) -> Option<usize> {
    let mut index = at + 1;

    while chars
        .get(index)
        .is_some_and(|char| char.is_alphanumeric() || *char == '_')
    {
        index += 1;
    }

    if chars.get(index) != Some(&'$') {
        return None;
    }

    let tag = chars[at..=index].iter().collect::<String>();

    Some(skip_until(chars, index + 1, &tag))
}

fn bounded(template: &str) -> String {
    if template.len() <= MAX_TEMPLATE_BYTES {
        return template.to_owned();
    }

    let mut end = MAX_TEMPLATE_BYTES - '…'.len_utf8();

    while !template.is_char_boundary(end) {
        end -= 1;
    }

    format!("{}…", &template[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(statement: &str) -> Option<String> {
        sql_template(statement, "postgresql")
    }

    #[test]
    fn literals_become_marks_and_lists_one_mark() {
        assert_eq!(
            template("SELECT * FROM orders WHERE id = 42 AND email = 'ada@example.com'").as_deref(),
            Some("SELECT * FROM orders WHERE id = ? AND email = ?"),
        );
        assert_eq!(
            template("select a from t where id in (1, 2, 3) and x = -1.5e3").as_deref(),
            Some("select a from t where id in (?) and x = -?"),
        );
        assert_eq!(
            template("INSERT INTO t (a, b) VALUES (1, 'x'), (2, 'it''s')").as_deref(),
            Some("INSERT INTO t (a, b) VALUES (?)"),
        );
    }

    #[test]
    fn names_placeholders_and_quoted_names_stay() {
        assert_eq!(
            template("UPDATE \"order_2024\" SET v = $1 WHERE t1.id = :id AND x = 0x1F").as_deref(),
            Some("UPDATE \"order_2024\" SET v = $1 WHERE t1.id = :id AND x = ?"),
        );
        assert_eq!(
            template("SELECT `user name` FROM users2 WHERE n = E'a\\'b'").as_deref(),
            Some("SELECT `user name` FROM users2 WHERE n = ?"),
        );
    }

    #[test]
    fn double_quotes_are_values_where_they_name_nothing() {
        assert_eq!(
            sql_template("SELECT id FROM users WHERE name = \"Ada\"", "mysql").as_deref(),
            Some("SELECT id FROM users WHERE name = ?"),
        );
        assert_eq!(
            sql_template("SELECT \"name\" FROM users", "PostgreSQL").as_deref(),
            Some("SELECT \"name\" FROM users"),
        );
    }

    #[test]
    fn comments_blanks_and_dollar_quotes_go() {
        assert_eq!(
            template("/* app=shop */ SELECT 1 -- trailing\n  FROM   dual").as_deref(),
            Some("SELECT ? FROM dual"),
        );
        assert_eq!(
            template("SELECT $body$ secret $body$, $$x$$ FROM t").as_deref(),
            Some("SELECT ? FROM t"),
        );
        assert_eq!(
            template("(SELECT 1) UNION (SELECT 2)").as_deref(),
            Some("(SELECT ?) UNION (SELECT ?)"),
        );
    }

    #[test]
    fn what_is_not_sql_is_not_kept() {
        assert_eq!(template("GET user:42"), None);
        assert_eq!(template("{\"find\": \"orders\"}"), None);
        assert_eq!(template(""), None);
    }

    #[test]
    fn a_long_template_is_cut() {
        let long = format!("SELECT {} FROM t", vec!["column_name"; 100].join(", "));
        let template = template(&long).expect("template");

        assert!(template.len() <= MAX_TEMPLATE_BYTES);
        assert!(template.ends_with('…'));
    }
}
