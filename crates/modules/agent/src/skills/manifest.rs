// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `SKILL.md` 的 YAML frontmatter：`name`、`description`，可选 `allowed-tools`、
//! `argument-hint`、`version`、`tags`、`examples`、`requires`。其余字段（`license`、
//! `metadata` 等）照样接受，`metadata.version` 在没有 `version` 时当作版本。

use serde::{Deserialize, Serialize};
use serde_yaml::Value;

use crate::shared::{Error, Result};

/// 名称长度上限。
pub const MAX_NAME_CHARS: usize = 64;

/// 说明长度上限，与开放格式一致。
pub const MAX_DESCRIPTION_CHARS: usize = 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillManifest {
    pub name: String,
    pub description: String,
    /// 本轮直接可见的工具；仍受组织工具策略与审批约束。
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub argument_hint: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// 「试试这样问」。
    #[serde(default)]
    pub examples: Vec<String>,
    /// 要跑脚本，只能在 Runner 上用。
    #[serde(default)]
    pub requires_runner: bool,
}

/// 名称：字母开头，字母、数字、`-`、`_`，1–64 个字符。
pub fn validate_name(name: &str) -> Result<()> {
    let mut chars = name.chars();

    let valid = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        && name.chars().count() <= MAX_NAME_CHARS;

    if valid {
        Ok(())
    } else {
        Err(Error::invalid(format!(
            "skill name `{name}` must start with a letter and use only letters, digits, `-` and `_` (at most {MAX_NAME_CHARS})"
        )))
    }
}

fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.trim().to_string()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
    .filter(|text| !text.is_empty())
}

/// 列表字段既可以写成 YAML 列表，也可以写成用逗号或空白分开的一行。
fn list(value: Option<&Value>) -> Vec<String> {
    let items: Vec<String> = match value {
        Some(Value::Sequence(items)) => items.iter().filter_map(text).collect(),
        Some(value) => text(value)
            .map(|line| {
                line.split(|c: char| c == ',' || c.is_whitespace())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        None => Vec::new(),
    };

    let mut seen = Vec::new();

    for item in items.into_iter().map(|item| item.trim().to_string()) {
        if !item.is_empty() && !seen.contains(&item) {
            seen.push(item);
        }
    }

    seen
}

/// 拆出 frontmatter 与正文。
fn split_front_matter(text: &str) -> Result<(&str, &str)> {
    let text = text.trim_start_matches('\u{feff}');

    let rest = text
        .strip_prefix("---\r\n")
        .or_else(|| text.strip_prefix("---\n"))
        .ok_or_else(|| Error::invalid("SKILL.md must start with a `---` frontmatter block"))?;

    let mut offset = 0;

    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return Ok((&rest[..offset], &rest[offset + line.len()..]));
        }

        offset += line.len();
    }

    if rest[offset..].trim_end() == "---" {
        return Ok((&rest[..offset], ""));
    }

    Err(Error::invalid(
        "the frontmatter of SKILL.md is not closed with `---`",
    ))
}

/// 读 `SKILL.md`：清单与去掉 frontmatter 的正文。
pub fn parse_skill_md(content: &str) -> Result<(SkillManifest, String)> {
    let (front, body) = split_front_matter(content)?;

    let yaml: Value = serde_yaml::from_str(front)
        .map_err(|error| Error::invalid(format!("the frontmatter of SKILL.md: {error}")))?;

    let field = |key: &str| yaml.get(key);

    let name = field("name")
        .and_then(text)
        .ok_or_else(|| Error::invalid("SKILL.md needs a `name`"))?;

    validate_name(&name)?;

    let description = field("description")
        .and_then(text)
        .ok_or_else(|| Error::invalid("SKILL.md needs a `description`"))?;

    if description.chars().count() > MAX_DESCRIPTION_CHARS {
        return Err(Error::invalid(format!(
            "the description of a skill holds at most {MAX_DESCRIPTION_CHARS} characters"
        )));
    }

    let version = field("version").and_then(text).or_else(|| {
        field("metadata")
            .and_then(|meta| meta.get("version"))
            .and_then(text)
    });

    let requires_runner = list(field("requires"))
        .iter()
        .any(|item| item.eq_ignore_ascii_case("runner"));

    let manifest = SkillManifest {
        name,
        description,
        allowed_tools: list(field("allowed-tools").or_else(|| field("allowed_tools"))),
        argument_hint: field("argument-hint")
            .or_else(|| field("argument_hint"))
            .and_then(text),
        version,
        tags: list(field("tags")),
        examples: match field("examples") {
            Some(Value::Sequence(items)) => items.iter().filter_map(text).collect(),
            Some(value) => text(value).into_iter().collect(),
            None => Vec::new(),
        },
        requires_runner,
    };

    Ok((manifest, body.trim_start_matches(['\r', '\n']).to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_open_format_and_its_optional_fields() {
        let (manifest, body) = parse_skill_md(
            "---\nname: incident-triage\ndescription: Triage an incident.\nallowed-tools: query_logs, get_incident\nargument-hint: <incident>\nversion: 1.2\ntags: [incidents, sre]\nexamples:\n  - Triage the API incident\nrequires: runner\nlicense: MIT\n---\n\n# Triage\nSteps.\n",
        )
        .unwrap();

        assert_eq!(manifest.name, "incident-triage");
        assert_eq!(manifest.allowed_tools, vec!["query_logs", "get_incident"]);
        assert_eq!(manifest.argument_hint.as_deref(), Some("<incident>"));
        assert_eq!(manifest.version.as_deref(), Some("1.2"));
        assert_eq!(manifest.tags, vec!["incidents", "sre"]);
        assert_eq!(manifest.examples, vec!["Triage the API incident"]);
        assert!(manifest.requires_runner);
        assert_eq!(body, "# Triage\nSteps.\n");
    }

    #[test]
    fn takes_the_version_from_metadata_and_tools_from_a_list() {
        let (manifest, _) = parse_skill_md(
            "---\r\nname: x\r\ndescription: y\r\nallowed-tools:\r\n  - list_streams\r\nmetadata:\r\n  version: \"2\"\r\n---\r\n",
        )
        .unwrap();

        assert_eq!(manifest.version.as_deref(), Some("2"));
        assert_eq!(manifest.allowed_tools, vec!["list_streams"]);
        assert!(!manifest.requires_runner);
    }

    #[test]
    fn refuses_what_is_not_a_skill() {
        assert!(parse_skill_md("# no frontmatter").is_err());
        assert!(parse_skill_md("---\nname: x\n").is_err());
        assert!(parse_skill_md("---\ndescription: y\n---\n").is_err());
        assert!(parse_skill_md("---\nname: x\n---\n").is_err());
        assert!(parse_skill_md("---\nname: 9lives\ndescription: y\n---\n").is_err());
        assert!(parse_skill_md("---\nname: [x\ndescription: y\n---\n").is_err());
    }

    #[test]
    fn names_start_with_a_letter() {
        assert!(validate_name("log-errors_2").is_ok());
        assert!(validate_name("").is_err());
        assert!(validate_name("-x").is_err());
        assert!(validate_name("a b").is_err());
        assert!(validate_name("排查").is_err());
        assert!(validate_name(&"a".repeat(65)).is_err());
    }
}
