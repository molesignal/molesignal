// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `MATCH_TEXT` 的查询语法：解析，改写成 `ILIKE` 表达式树。裁剪用的词从改写后的 `ILIKE` 里
//! 读，见 [`super::match_sql::like_predicates`]。

/// `MATCH_TEXT(field, 'query')` 查询语法（设计 D4，spec text-match-functions）解析后的 AST：
///
/// - 空格分隔的多个词 = token 级 AND（可分散出现）；
/// - `"..."` = 短语（引号内作为连续子串）；
/// - `*` = 通配符（前缀 / 后缀 / 包含 / 短语内），rewrite 时转为 ILIKE 的 `%`；
/// - `a OR b` = 或；
/// - `-a` = 排除（NOT）；
/// - `\` = 转义：`\*`、`\"`、`\\`（以及 `\%`、`\_`）表示字面量；
/// - 空 query 或全部为空 token = 恒 false（[`MatchTextQuery::AlwaysFalse`]）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MatchTextQuery {
    /// 空 query 或全部为空 token：恒不匹配任何行。
    AlwaysFalse,
    /// 空格分隔的 token 级合取（纯 AND 树）。
    Conjunction(Vec<MatchTextQuery>),
    /// `a OR b`。
    Or(Box<MatchTextQuery>, Box<MatchTextQuery>),
    /// `-a`：排除。
    Not(Box<MatchTextQuery>),
    /// 单个检索项（词或短语）。`like` 是已转义为 ILIKE pattern 的内容（不含外围 `%`，
    /// 短语时含内部空格）；`wildcard` 为 true 表示含未转义 `*`（已转成 `%`），此时该片段
    /// 本身就是完整 pattern（前缀 / 后缀 / 包含位置由用户指定），不能再加外围 `%`。
    Term { like: String, wildcard: bool },
}

/// 解析 `MATCH_TEXT` 查询。空串或全部为空 token → [`MatchTextQuery::AlwaysFalse`]。
pub(super) fn parse_match_text(query: &str) -> MatchTextQuery {
    let items = tokenize_match_text(query);

    // 顶层按独立 `OR` 关键字分组（短语 / 转义内的 `OR` 不受影响）。
    let groups: Vec<&[MatchTextItem]> = items
        .split(|item| matches!(item, MatchTextItem::Or))
        .collect();

    let branches: Vec<MatchTextQuery> = groups
        .iter()
        .map(|group| build_conjunction(group))
        .collect();

    combine_or(branches)
}

/// 把解析出的 AST 展开为 DataFusion 可执行的 ILIKE 表达式树（D1：执行语义）。
/// `field` 为 SQL 中的字段标识符（原样保留）。空 / 全空 query 展开为 `FALSE`。
pub(super) fn rewrite_match_text(query: &MatchTextQuery, field: &str) -> String {
    match query {
        MatchTextQuery::AlwaysFalse => "FALSE".to_string(),
        MatchTextQuery::Term { like, wildcard, .. } => {
            if *wildcard {
                // 用户显式给了 `*`（前缀/后缀/包含位置），该片段已是完整 pattern。
                format!("{field} ILIKE '{like}'")
            } else {
                // 普通词 / 短语：子串包含语义，包上外围 `%`。
                format!("{field} ILIKE '%{like}%'")
            }
        }
        MatchTextQuery::Conjunction(terms) => {
            let inner = terms
                .iter()
                .map(|term| rewrite_match_text(term, field))
                .collect::<Vec<_>>()
                .join(" AND ");

            format!("({inner})")
        }
        MatchTextQuery::Or(left, right) => format!(
            "({} OR {})",
            rewrite_match_text(left, field),
            rewrite_match_text(right, field)
        ),
        MatchTextQuery::Not(expr) => format!("NOT ({})", rewrite_match_text(expr, field)),
    }
}

/// 把单个字符转义为 ILIKE pattern 中的字面量。LIKE 的 `%` / `_` 通配符与 `\` 转义符
/// 前置 `\` 后按字面量匹配（arrow like 内核以 `\` 为默认转义，见
/// `arrow-string::like` 的 `like_escape` 测试）。
pub(super) fn escape_like_literal(c: char) -> String {
    match c {
        '%' => "\\%".to_string(),
        '_' => "\\_".to_string(),
        '\\' => "\\\\".to_string(),
        other => other.to_string(),
    }
}

/// 词法元素：单个词 / 引号短语 / 独立 `OR` 关键字。`negated` 表示元素前有 `-` 前缀。
#[derive(Debug, Clone)]
enum MatchTextItem {
    Term {
        like: String,
        wildcard: bool,
        negated: bool,
    },
    Phrase {
        like: String,
        wildcard: bool,
        negated: bool,
    },
    Or,
}

fn tokenize_match_text(query: &str) -> Vec<MatchTextItem> {
    let chars: Vec<char> = query.chars().collect();
    let mut items = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }

        if i >= chars.len() {
            break;
        }

        // `-` 前缀 = 排除（NOT），作用于紧随其后的词 / 短语。
        let negated = if chars[i] == '-' {
            i += 1;

            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }

            true
        } else {
            false
        };

        if i >= chars.len() {
            break; // 裸 `-`：没有可排除的目标，忽略。
        }

        if chars[i] == '"' {
            let (like, wildcard, next) = scan_phrase(&chars, i);

            items.push(MatchTextItem::Phrase {
                like,
                wildcard,
                negated,
            });

            i = next;
        } else if negated && is_or_keyword(&chars, i) {
            // `- OR ...`：`-` 退化为字面 token，`OR` 留待下一轮。
            items.push(MatchTextItem::Term {
                like: "-".to_string(),
                wildcard: false,
                negated: false,
            });
        } else if is_or_keyword(&chars, i) {
            items.push(MatchTextItem::Or);
            i += 2;
        } else {
            let (like, wildcard, next) = scan_token(&chars, i);

            items.push(MatchTextItem::Term {
                like,
                wildcard,
                negated,
            });

            i = next;
        }
    }

    items
}

/// 独立 `OR` 关键字（大小写不敏感）：前后必须是词边界，避免把 `orange` 这类词误判。
fn is_or_keyword(chars: &[char], i: usize) -> bool {
    if i + 2 > chars.len() {
        return false;
    }

    if !(chars[i].eq_ignore_ascii_case(&'o') && chars[i + 1].eq_ignore_ascii_case(&'r')) {
        return false;
    }

    chars
        .get(i + 2)
        .is_none_or(|c| c.is_whitespace() || *c == '"' || *c == '-')
}

/// 扫描单个词到空白 / 引号：`*` 转 `%`（通配符），`\X` 转义字面量，`%`/`_` 字面量转义。
/// 返回 (ILIKE pattern 片段, 是否含 `*` 通配符, 下一个位置)。
fn scan_token(chars: &[char], start: usize) -> (String, bool, usize) {
    let mut i = start;
    let mut like = String::new();
    let mut wildcard = false;

    while i < chars.len() {
        let c = chars[i];

        if c.is_whitespace() || c == '"' {
            break;
        }

        match c {
            '\\' => {
                i += 1;

                if let Some(next) = chars.get(i) {
                    like.push_str(&escape_like_literal(*next));
                    i += 1;
                } else {
                    // 结尾裸 `\`：字面反斜杠。
                    like.push_str("\\\\");
                }
            }
            '*' => {
                like.push('%');
                wildcard = true;
                i += 1;
            }
            '%' | '_' => {
                like.push_str(&escape_like_literal(c));
                i += 1;
            }
            _ => {
                like.push(c);
                i += 1;
            }
        }
    }

    (like, wildcard, i)
}

/// 扫描引号短语：引号内文本作为连续子串（含空白与内部通配符），`\X` 转义字面量。
/// 未闭合的引号视作短语延伸到行末，保持可检索性。
/// 返回 (ILIKE pattern 片段, 是否含 `*` 通配符, 下一个扫描位置)。
fn scan_phrase(chars: &[char], start: usize) -> (String, bool, usize) {
    let mut i = start + 1;
    let mut like = String::new();
    let mut wildcard = false;

    while i < chars.len() {
        match chars[i] {
            '\\' => {
                i += 1;

                if let Some(next) = chars.get(i) {
                    like.push_str(&escape_like_literal(*next));
                    i += 1;
                } else {
                    like.push_str("\\\\");
                }
            }
            '"' => {
                i += 1;
                break;
            }
            '*' => {
                like.push('%');
                wildcard = true;
                i += 1;
            }
            '%' | '_' => {
                like.push_str(&escape_like_literal(chars[i]));
                i += 1;
            }
            c => {
                like.push(c);
                i += 1;
            }
        }
    }

    (like, wildcard, i)
}

/// 单个 OR 分组（一个合取段）→ 合取表达式；全空 → [`MatchTextQuery::AlwaysFalse`]。
fn build_conjunction(items: &[MatchTextItem]) -> MatchTextQuery {
    let terms: Vec<MatchTextQuery> = items
        .iter()
        .map(|item| match item {
            MatchTextItem::Term {
                like,
                wildcard,
                negated,
            }
            | MatchTextItem::Phrase {
                like,
                wildcard,
                negated,
            } => {
                let expr = MatchTextQuery::Term {
                    like: like.clone(),
                    wildcard: *wildcard,
                };

                if *negated {
                    MatchTextQuery::Not(Box::new(expr))
                } else {
                    expr
                }
            }
            MatchTextItem::Or => unreachable!("Or 已在顶层分组时消费"),
        })
        .collect();

    match terms.len() {
        0 => MatchTextQuery::AlwaysFalse,
        1 => terms.into_iter().next().expect("len == 1"),
        _ => MatchTextQuery::Conjunction(terms),
    }
}

/// 把多个 OR 分支结合为 Or 树；全部分支为空 → [`MatchTextQuery::AlwaysFalse`]。
fn combine_or(branches: Vec<MatchTextQuery>) -> MatchTextQuery {
    let mut rest: Vec<MatchTextQuery> = branches
        .into_iter()
        .filter(|branch| !matches!(branch, MatchTextQuery::AlwaysFalse))
        .collect();

    match rest.len() {
        0 => MatchTextQuery::AlwaysFalse,
        1 => rest.pop().expect("len == 1"),
        _ => {
            let mut iter = rest.into_iter();
            let mut acc = iter.next().expect("len >= 2");

            for next in iter {
                acc = MatchTextQuery::Or(Box::new(acc), Box::new(next));
            }

            acc
        }
    }
}
