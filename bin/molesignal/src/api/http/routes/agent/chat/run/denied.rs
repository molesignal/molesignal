// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Built-in tools a run may not call, taken out of what its toolsets allow.
//!
//! A drill's run may not read what people found after the incident it investigates again
//! ([`DENIED_TOOLS`]). The allowed set narrows: a denied tool is neither shown to the model, found
//! by `tool_search`, nor run through `tools_call`, since all of them ask the resolution whether a
//! tool is enabled.

use agent::{
    runs::{Run, RunSource},
    scoring::drills::DENIED_TOOLS,
};
use tool_runtime::catalog::builtin_tools;

use crate::api::http::routes::agent::toolsets::ToolsetResolution;

/// Narrows the resolution for the run: a drill loses [`DENIED_TOOLS`]; other runs keep theirs.
pub(super) fn for_run(run: &Run, resolution: &mut ToolsetResolution) {
    if run.source == RunSource::Drill {
        deny_tools(resolution, &DENIED_TOOLS);
    }
}

/// Takes `denied` out of the built-in tools the resolution allows.
fn deny_tools(resolution: &mut ToolsetResolution, denied: &[&str]) {
    if denied.is_empty() {
        return;
    }

    let allowed = resolution
        .builtin_allowed
        .get_or_insert_with(|| builtin_tools().into_iter().map(|tool| tool.name).collect());

    for name in denied {
        allowed.remove(*name);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn denied_tools_are_no_longer_enabled() {
        let mut resolution = ToolsetResolution::default();
        assert!(resolution.builtin_enabled("get_incident_rca"));

        deny_tools(&mut resolution, &DENIED_TOOLS);

        for name in DENIED_TOOLS {
            assert!(!resolution.builtin_enabled(name), "{name}");
        }

        assert!(resolution.builtin_enabled("query_logs"));
        assert!(resolution.builtin_enabled("list_changes"));
        assert!(resolution.builtin_enabled("tools_call"));
    }

    #[test]
    fn a_narrowed_set_stays_narrow() {
        let mut resolution = ToolsetResolution {
            builtin_allowed: Some(HashSet::from(["query_logs".to_string()])),
            ..ToolsetResolution::default()
        };

        deny_tools(&mut resolution, &["get_incident"]);

        assert_eq!(
            resolution.builtin_allowed,
            Some(HashSet::from(["query_logs".to_string()]))
        );

        deny_tools(&mut resolution, &[]);
        assert!(resolution.builtin_enabled("query_logs"));
    }

    #[test]
    fn every_denied_tool_is_a_built_in_tool() {
        let names = builtin_tools()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<HashSet<_>>();

        for name in DENIED_TOOLS {
            assert!(names.contains(name), "{name} is not a built-in tool");
        }
    }
}
