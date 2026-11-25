// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The spec of each task kind, checked as the node checks it: the keys a kind takes and no other.
//!
//! - `tool`: `node_observe::tools::ToolSpec` (dropwatch, tcpshark, iotracing, perf).
//! - `autotracing_run`: `{"rule", "container_id"?}`, one of the AutoTracing scenarios run once now;
//!   `container_id` only for cpuidle and dload (without it the node picks the container).
//! - `profile`: perf's options without `tool` (`duration_secs`, `pid` | `container_id`,
//!   `frequency_hz`, `mode`, `phase`, `min_duration_us`, `probability`, `language`); the node runs it
//!   as perf.

use kernel::{Error, Result};
use serde_json::{Map, Value};

use super::TaskKind;

/// The AutoTracing scenarios an `autotracing_run` can name.
pub const AUTOTRACING_RULES: [&str; 5] = ["cpusys", "cpuidle", "dload", "memburst", "iotracing"];

/// The scenarios that run against one container, given by `container_id` or picked by the node.
pub const CONTAINER_RULES: [&str; 2] = ["cpuidle", "dload"];

/// The languages `perf` and `profile` take: the native profiler or a runtime's.
pub const PROFILE_LANGUAGES: [&str; 3] = ["native", "java", "python"];

/// The options of a `profile`; `perf` takes them too, with `tool`.
const PROFILE_KEYS: [&str; 9] = [
    "duration_secs",
    "pid",
    "container_id",
    "frequency_hz",
    "mode",
    "phase",
    "min_duration_us",
    "probability",
    "language",
];

/// The native profile modes of `perf` and `profile`.
pub const PROFILE_MODES: [&str; 5] = [
    "oncpu",
    "offcpu",
    "virtual_alloc",
    "physical_alloc",
    "physical_usage",
];

const MAX_SPEC_BYTES: usize = 64 * 1024;

fn invalid(message: impl Into<String>) -> Error {
    Error::invalid(message)
}

fn only_keys(object: &Map<String, Value>, allowed: &[&str], what: &str) -> Result<()> {
    match object.keys().find(|key| !allowed.contains(&key.as_str())) {
        Some(key) => Err(invalid(format!("`{key}` is not an option of {what}"))),
        None => Ok(()),
    }
}

fn uint(object: &Map<String, Value>, key: &str) -> Result<Option<u64>> {
    match object.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| invalid(format!("`{key}` must be a whole number"))),
    }
}

fn string<'a>(object: &'a Map<String, Value>, key: &str) -> Result<Option<&'a str>> {
    match object.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_str()
            .map(Some)
            .ok_or_else(|| invalid(format!("`{key}` must be a string"))),
    }
}

fn strings<'a>(object: &'a Map<String, Value>, key: &str) -> Result<Vec<&'a str>> {
    match object.get(key) {
        None => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .ok_or_else(|| invalid(format!("`{key}` must be an array of strings")))
            })
            .collect(),
        Some(_) => Err(invalid(format!("`{key}` must be an array of strings"))),
    }
}

/// `duration_secs` from 1 to 3600, required.
fn duration(object: &Map<String, Value>) -> Result<()> {
    match uint(object, "duration_secs")? {
        Some(1..=3600) => Ok(()),
        _ => Err(invalid("`duration_secs` must be between 1 and 3600")),
    }
}

/// `pid` or `container_id`, not both; neither is the whole host.
fn target(object: &Map<String, Value>) -> Result<()> {
    let pid = uint(object, "pid")?;
    let container = string(object, "container_id")?.filter(|id| !id.is_empty());

    if pid == Some(0) {
        return Err(invalid("`pid` must be above 0"));
    }

    if pid.is_some() && container.is_some() {
        return Err(invalid("`pid` and `container_id` are mutually exclusive"));
    }

    Ok(())
}

/// The profile options of `perf` and `profile`: a target, and a mode of the native profiler unless
/// `language` names a runtime (java, python), whose modes the node checks itself.
fn native_profile(object: &Map<String, Value>) -> Result<()> {
    target(object)?;
    duration(object)?;

    for key in ["frequency_hz", "min_duration_us", "probability"] {
        uint(object, key)?;
    }

    let language = string(object, "language")?.unwrap_or("native");

    if !PROFILE_LANGUAGES.contains(&language) {
        return Err(invalid(format!(
            "`language` must be one of {}",
            PROFILE_LANGUAGES.join(", ")
        )));
    }

    if language != "native" {
        return match ["phase", "min_duration_us", "probability"]
            .into_iter()
            .find(|key| object.contains_key(*key))
        {
            Some(key) => Err(invalid(format!(
                "`{key}` does not apply to {language} profiling"
            ))),
            None => Ok(()),
        };
    }

    if let Some(mode) = string(object, "mode")?
        && !PROFILE_MODES.contains(&mode)
    {
        return Err(invalid(format!(
            "`mode` {mode:?} is not one of {}",
            PROFILE_MODES.join(", ")
        )));
    }

    match string(object, "phase")? {
        None | Some("" | "all" | "blocked" | "runqueue") => Ok(()),
        Some(phase) => Err(invalid(format!(
            "`phase` {phase:?} is not one of all, blocked and runqueue"
        ))),
    }
}

fn tool(object: &Map<String, Value>) -> Result<()> {
    let name = string(object, "tool")?.ok_or_else(|| invalid("a tool spec needs `tool`"))?;

    let keys: &[&str] = match name {
        "dropwatch" => &[
            "tool",
            "filter",
            "duration_secs",
            "max_events_per_second",
            "devices",
            "devices_excluded",
        ],
        "tcpshark" => &[
            "tool",
            "filter",
            "duration_secs",
            "max_events_per_second",
            "mode",
            "enable_tlp",
        ],
        "iotracing" => &[
            "tool",
            "duration_secs",
            "devices",
            "max_stack",
            "max_process",
            "max_files_per_process",
            "schedule_threshold_ms",
        ],
        "perf" => {
            if let Some(key) = object
                .keys()
                .find(|key| *key != "tool" && !PROFILE_KEYS.contains(&key.as_str()))
            {
                return Err(invalid(format!("`{key}` is not an option of perf")));
            }

            return native_profile(object);
        }
        _ => {
            return Err(invalid(
                "`tool` must be one of dropwatch, tcpshark, iotracing and perf",
            ));
        }
    };

    only_keys(object, keys, name)?;
    duration(object)?;
    string(object, "filter")?;

    for key in [
        "max_events_per_second",
        "max_stack",
        "max_process",
        "max_files_per_process",
        "schedule_threshold_ms",
    ] {
        uint(object, key)?;
    }

    match name {
        "dropwatch" => {
            if !strings(object, "devices")?.is_empty()
                && !strings(object, "devices_excluded")?.is_empty()
            {
                return Err(invalid(
                    "`devices` and `devices_excluded` are mutually exclusive",
                ));
            }
        }
        "tcpshark" => {
            if string(object, "mode")?.is_some_and(|mode| !mode.is_empty() && mode != "retransmit")
            {
                return Err(invalid("tcpshark `mode` must be \"retransmit\""));
            }

            if object
                .get("enable_tlp")
                .is_some_and(|value| !value.is_boolean())
            {
                return Err(invalid("`enable_tlp` must be a boolean"));
            }
        }
        "iotracing" => {
            let major_minor = |device: &&str| {
                device.split_once(':').is_some_and(|(major, minor)| {
                    major.parse::<u32>().is_ok() && minor.parse::<u32>().is_ok()
                })
            };

            if !strings(object, "devices")?.iter().all(major_minor) {
                return Err(invalid("iotracing `devices` are major:minor, e.g. 259:3"));
            }
        }
        _ => {}
    }

    Ok(())
}

/// Checks the spec of a new task the way the node will.
pub fn validate_spec(kind: TaskKind, spec: &Value) -> Result<()> {
    let Some(object) = spec.as_object() else {
        return Err(invalid("a task spec must be a JSON object"));
    };

    if serde_json::to_vec(spec).map_or(usize::MAX, |bytes| bytes.len()) > MAX_SPEC_BYTES {
        return Err(invalid("a task spec must stay within 64 KiB"));
    }

    match kind {
        TaskKind::Tool => tool(object),
        TaskKind::AutotracingRun => {
            only_keys(object, &["rule", "container_id"], "an AutoTracing run")?;

            let rule = match string(object, "rule")? {
                Some(rule) if AUTOTRACING_RULES.contains(&rule) => rule,
                _ => {
                    return Err(invalid(format!(
                        "`rule` must be one of {}",
                        AUTOTRACING_RULES.join(", ")
                    )));
                }
            };

            // Only the rules that pick a container take one; without it the node picks the busiest.
            match string(object, "container_id")? {
                Some("") => Err(invalid("`container_id` cannot be empty")),
                Some(_) if !CONTAINER_RULES.contains(&rule) => {
                    Err(invalid(format!("rule {rule} has no container target")))
                }
                _ => Ok(()),
            }
        }
        // perf's options without `tool`: the node runs it as perf.
        TaskKind::Profile => {
            only_keys(object, &PROFILE_KEYS, "a profile")?;
            native_profile(object)
        }
    }
}
