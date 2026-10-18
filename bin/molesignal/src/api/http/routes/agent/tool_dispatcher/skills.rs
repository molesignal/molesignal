// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `list_skills`、`load_skill`、`read_skill_file`：只在这个会话能用的 Skill 里找。名称写法与
//! 清单里一样，团队 Skill 有歧义时带 `@团队`。

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    agent::{
        skills::wrap_skill_body,
        tools::{BuiltinToolKind, ToolResult},
    },
    api::AppState,
    app::skills::{Lookup, SessionSkill, SessionSkills, text_file},
    shared::{Error, Result},
};

#[derive(Debug, Deserialize)]
struct SkillArgs {
    name: String,
    #[serde(default)]
    path: Option<String>,
}

fn find<'a>(skills: &'a SessionSkills, raw: &str) -> std::result::Result<&'a SessionSkill, String> {
    let raw = raw.trim().trim_start_matches('/');

    let (name, team) = match raw.split_once('@') {
        Some((name, team)) => (name, Some(team)),
        None => (raw, None),
    };

    match skills.find(name, team) {
        Lookup::Found(skill) => Ok(skill),
        Lookup::Ambiguous(options) => Err(format!(
            "`{raw}` is ambiguous in this chat; use one of: {}",
            options.join(", ")
        )),
        Lookup::Missing => Err(format!(
            "no skill named `{raw}` is available in this chat; call list_skills to see them"
        )),
    }
}

pub(super) async fn execute(
    state: &AppState,
    skills: &SessionSkills,
    kind: BuiltinToolKind,
    arguments: Value,
) -> Result<ToolResult> {
    if kind == BuiltinToolKind::ListSkills {
        return Ok(ToolResult::json(json!({
            "skills": skills.skills.iter().map(|skill| json!({
                "name": skill.invocation,
                "description": skill.description,
                "version": skill.version,
                "argument_hint": skill.manifest.argument_hint,
                "source": skill.source,
            })).collect::<Vec<_>>()
        })));
    }

    let args: SkillArgs = serde_json::from_value(arguments)
        .map_err(|error| Error::invalid(format!("invalid tool arguments: {error}")))?;

    let skill = match find(skills, &args.name) {
        Ok(skill) => skill,
        Err(message) => return Ok(ToolResult::error(message)),
    };

    let bundle = state.agent.skills.bundle(&skill.location).await?;

    match kind {
        BuiltinToolKind::LoadSkill => Ok(ToolResult::json(json!({
            "name": skill.invocation,
            "version": skill.version,
            "instructions": wrap_skill_body(&skill.invocation, skill.version.as_deref(), &bundle.body),
            "files": bundle.index(),
        }))),
        BuiltinToolKind::ReadSkillFile => {
            let path = args.path.unwrap_or_default();

            match text_file(&bundle, &path) {
                Ok(content) => Ok(ToolResult::json(json!({
                    "name": skill.invocation,
                    "path": path,
                    "content": content,
                }))),
                Err(error) => Ok(ToolResult::error(error.to_string())),
            }
        }
        _ => Ok(ToolResult::error("not a skill tool")),
    }
}
