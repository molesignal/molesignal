// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 模板里的条件块：
//!
//! - `{{#if 变量}}…{{/if}}`：变量有值（不是空的）时留下中间的内容。
//! - `{{#if 变量 == "值"}}`、`{{#if 变量 != "值"}}`：按字符串比较，值写在引号里。
//! - `{{else}}`：条件不成立时留下的内容。
//!
//! 可以嵌套，最多 [`MAX_DEPTH`] 层。条件块先于占位符处理：留下的内容里的 `{{变量}}` 照常填。网页的
//! 模板预览按同样的规则渲染（`web/src/routes/notify/template/blocks.ts`）。

use std::collections::BTreeMap;

use crate::shared::{Error, Result};

/// 条件块最多嵌套几层。
pub(super) const MAX_DEPTH: usize = 5;

/// 一段 `{{…}}` 里是什么。
#[derive(Debug, PartialEq, Eq)]
enum Tag<'a> {
    If(Condition<'a>),
    Else,
    EndIf,
    /// 别的 `{{…}}`：占位符，原样留给后面填。
    Other,
}

#[derive(Debug, PartialEq, Eq)]
enum Condition<'a> {
    Present(&'a str),
    Equals(&'a str, &'a str),
    Differs(&'a str, &'a str),
}

impl Condition<'_> {
    fn holds(&self, values: &BTreeMap<String, String>) -> bool {
        let value = |path: &str| values.get(path).map(String::as_str).unwrap_or_default();

        match self {
            Self::Present(path) => !value(path).trim().is_empty(),
            Self::Equals(path, expected) => value(path) == *expected,
            Self::Differs(path, expected) => value(path) != *expected,
        }
    }
}

/// 一个条件块走到哪儿了。
struct Frame {
    /// 外层都成立时，这一块当前的分支是否留下。
    showing: bool,
    /// 这一块的条件是否成立。
    held: bool,
    /// 已经到 `{{else}}` 了。
    in_else: bool,
}

/// 按 `values` 去掉不成立的分支，其余（包括占位符）原样留下。写法不对时原样返回，保存时由
/// [`validate_blocks`] 挡住。
pub(super) fn resolve_blocks(body: &str, values: &BTreeMap<String, String>) -> String {
    if !body.contains("{{#") {
        return body.to_string();
    }

    resolve(body, values).unwrap_or_else(|_| body.to_string())
}

/// 存模板前的检查：块要配对，嵌套不超过 [`MAX_DEPTH`] 层，条件写法对。
pub(super) fn validate_blocks(body: &str) -> Result<()> {
    resolve(body, &BTreeMap::new()).map(drop)
}

fn resolve(body: &str, values: &BTreeMap<String, String>) -> Result<String> {
    let mut output = String::with_capacity(body.len());
    let mut stack: Vec<Frame> = Vec::new();
    let mut rest = body;

    let visible = |stack: &[Frame]| stack.iter().all(|frame| frame.showing);

    while let Some(start) = rest.find("{{") {
        if visible(&stack) {
            output.push_str(&rest[..start]);
        }

        let after = &rest[start + 2..];

        let Some(end) = after.find("}}") else {
            if visible(&stack) {
                output.push_str(&rest[start..]);
            }

            rest = "";
            break;
        };

        let raw = &after[..end];

        match parse_tag(raw.trim())? {
            Tag::If(condition) => {
                if stack.len() >= MAX_DEPTH {
                    return Err(Error::invalid(format!(
                        "template conditions nest at most {MAX_DEPTH} deep"
                    )));
                }

                let held = condition.holds(values);

                stack.push(Frame {
                    showing: held,
                    held,
                    in_else: false,
                });
            }
            Tag::Else => {
                let frame = stack
                    .last_mut()
                    .ok_or_else(|| Error::invalid("`{{else}}` outside an `{{#if}}` block"))?;

                if frame.in_else {
                    return Err(Error::invalid("an `{{#if}}` block has two `{{else}}`"));
                }

                frame.in_else = true;
                frame.showing = !frame.held;
            }
            Tag::EndIf => {
                stack
                    .pop()
                    .ok_or_else(|| Error::invalid("`{{/if}}` without an `{{#if}}` block"))?;
            }
            Tag::Other => {
                if visible(&stack) {
                    output.push_str("{{");
                    output.push_str(raw);
                    output.push_str("}}");
                }
            }
        }

        rest = &after[end + 2..];
    }

    if !stack.is_empty() {
        return Err(Error::invalid(
            "an `{{#if}}` block is not closed with `{{/if}}`",
        ));
    }

    output.push_str(rest);
    Ok(output)
}

fn parse_tag(tag: &str) -> Result<Tag<'_>> {
    if tag == "else" {
        return Ok(Tag::Else);
    }

    if tag == "/if" {
        return Ok(Tag::EndIf);
    }

    let Some(condition) = tag.strip_prefix('#') else {
        return Ok(Tag::Other);
    };

    let condition = condition
        .strip_prefix("if")
        .filter(|rest| rest.starts_with(char::is_whitespace))
        .map(str::trim)
        .ok_or_else(|| {
            Error::invalid(format!(
                "`{{{{{tag}}}}}` is not a condition: write `{{{{#if …}}}}`"
            ))
        })?;

    if let Some((path, value)) = condition.split_once("==") {
        return Ok(Tag::If(Condition::Equals(variable(path)?, quoted(value)?)));
    }

    if let Some((path, value)) = condition.split_once("!=") {
        return Ok(Tag::If(Condition::Differs(variable(path)?, quoted(value)?)));
    }

    Ok(Tag::If(Condition::Present(variable(condition)?)))
}

fn variable(path: &str) -> Result<&str> {
    let path = path.trim();
    let valid = !path.is_empty()
        && path
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '-'));

    if valid {
        Ok(path)
    } else {
        Err(Error::invalid(format!(
            "`{path}` is not a variable a condition can test"
        )))
    }
}

fn quoted(value: &str) -> Result<&str> {
    let value = value.trim();

    value
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|inner| inner.strip_suffix('\''))
        })
        .ok_or_else(|| Error::invalid(format!("compare with a quoted value, not `{value}`")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn a_condition_keeps_its_branch_and_leaves_placeholders_for_later() {
        let body =
            r#"{{#if incident.status == "resolved"}}已恢复{{else}}请处理 {{summary}}{{/if}}"#;

        assert_eq!(
            resolve_blocks(body, &values(&[("incident.status", "resolved")])),
            "已恢复"
        );
        assert_eq!(
            resolve_blocks(body, &values(&[("incident.status", "open")])),
            "请处理 {{summary}}"
        );
    }

    #[test]
    fn presence_and_difference_and_nesting() {
        let body = "A{{#if runbook}}[{{runbook}}]{{#if severity != 'info'}}!{{/if}}{{/if}}B";

        assert_eq!(resolve_blocks(body, &values(&[])), "AB");
        assert_eq!(
            resolve_blocks(
                body,
                &values(&[("runbook", "https://r"), ("severity", "info")])
            ),
            "A[{{runbook}}]B"
        );
        assert_eq!(
            resolve_blocks(
                body,
                &values(&[("runbook", "https://r"), ("severity", "critical")])
            ),
            "A[{{runbook}}]!B"
        );
        // 只有空白也算没有值。
        assert_eq!(resolve_blocks(body, &values(&[("runbook", "  ")])), "AB");
    }

    #[test]
    fn mistakes_are_caught_when_the_template_is_saved() {
        assert!(validate_blocks("{{#if a}}x").is_err());
        assert!(validate_blocks("x{{/if}}").is_err());
        assert!(validate_blocks("{{else}}").is_err());
        assert!(validate_blocks("{{#if a}}1{{else}}2{{else}}3{{/if}}").is_err());
        assert!(
            validate_blocks("{{#if a == b}}x{{/if}}").is_err(),
            "unquoted value"
        );
        assert!(validate_blocks("{{#each a}}x{{/if}}").is_err());
        assert!(validate_blocks("{{#if a b}}x{{/if}}").is_err());
        assert!(validate_blocks(&"{{#if a}}".repeat(MAX_DEPTH + 1)).is_err());

        let deepest = format!(
            "{}x{}",
            "{{#if a}}".repeat(MAX_DEPTH),
            "{{/if}}".repeat(MAX_DEPTH)
        );
        assert!(validate_blocks(&deepest).is_ok());
        assert!(validate_blocks("plain {{summary}}").is_ok());
    }
}
