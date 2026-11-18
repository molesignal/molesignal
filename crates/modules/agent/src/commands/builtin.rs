// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Built-in commands, all corresponding to existing capabilities.
//! Descriptions and argument hints are human-readable text stored in the `i18n_language` table
//! (`agent.command.<name>.description` and `.argument`) in the reader's interface language.

use serde::{Deserialize, Serialize};

use super::CommandKind;

/// A command's argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Argument {
    None,
    /// Required: a word from `choices`, or a piece of text.
    Required,
}

/// The frontend retrieves argument suggestions from existing objects in the organization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Suggest {
    Models,
    Profiles,
    Prompts,
    Incidents,
    /// Teams the current user belongs to (all teams for administrators).
    Teams,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinCommand {
    pub name: &'static str,
    pub kind: CommandKind,
    pub argument: Argument,
    /// The argument must be one of these words; empty means unrestricted.
    pub choices: &'static [&'static str],
    pub suggest: Option<Suggest>,
    /// The permission required in addition to `agent.use`.
    pub permission: Option<&'static str>,
}

const fn control(name: &'static str) -> BuiltinCommand {
    BuiltinCommand {
        name,
        kind: CommandKind::Control,
        argument: Argument::None,
        choices: &[],
        suggest: None,
        permission: None,
    }
}

const fn choose(name: &'static str, choices: &'static [&'static str]) -> BuiltinCommand {
    BuiltinCommand {
        argument: Argument::Required,
        choices,
        ..control(name)
    }
}

const fn pick(name: &'static str, suggest: Suggest) -> BuiltinCommand {
    BuiltinCommand {
        argument: Argument::Required,
        suggest: Some(suggest),
        ..control(name)
    }
}

const fn task(name: &'static str) -> BuiltinCommand {
    BuiltinCommand {
        kind: CommandKind::Task,
        argument: Argument::Required,
        ..control(name)
    }
}

/// The order listed in the input box.
pub const BUILTIN_COMMANDS: &[BuiltinCommand] = &[
    control("help"),
    control("new"),
    pick("team", Suggest::Teams),
    choose("mode", &["auto", "quick", "deep", "query"]),
    choose("time", &["15m", "30m", "1h", "6h", "24h", "7d"]),
    choose(
        "policy",
        &["advice", "readonly", "approval", "auto", "bypass"],
    ),
    pick("model", Suggest::Models),
    pick("profile", Suggest::Profiles),
    pick("prompt", Suggest::Prompts),
    task("query"),
    task("dashboard"),
    BuiltinCommand {
        suggest: Some(Suggest::Incidents),
        permission: Some("alerts.incidents.read"),
        ..task("rca")
    },
    control("compact"),
    control("export"),
];

pub fn find_builtin(name: &str) -> Option<&'static BuiltinCommand> {
    BUILTIN_COMMANDS.iter().find(|command| command.name == name)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::commands::{Input, parse};

    #[test]
    fn every_name_is_unique_and_parses_back_to_itself() {
        let mut seen = HashSet::new();

        for command in BUILTIN_COMMANDS {
            assert!(seen.insert(command.name), "{} twice", command.name);

            match parse(&format!("/{} x", command.name)) {
                Input::Command(invocation) => assert_eq!(invocation.name, command.name),
                Input::Text(_) => panic!("/{} is not read as a command", command.name),
            }
        }
    }

    #[test]
    fn tasks_need_something_to_work_on() {
        for command in BUILTIN_COMMANDS
            .iter()
            .filter(|command| command.kind == CommandKind::Task)
        {
            assert_eq!(command.argument, Argument::Required, "{}", command.name);
        }

        assert_eq!(
            find_builtin("rca").unwrap().permission,
            Some("alerts.incidents.read")
        );
        assert!(find_builtin("RCA").is_none());
    }
}
