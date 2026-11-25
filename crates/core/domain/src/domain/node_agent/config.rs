// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The configuration the control plane pushes to a node (`ConfigUpdate.config`), schema v1.
//!
//! Each key is a part of the agent's standalone configuration (molesignal-node-agent
//! `bin/node-agent/src/config.rs` and `config_options.rs`): which collectors run, the dropwatch
//! filter, the memory reclaim threshold and continuous native profiling. A key left out keeps what
//! the agent has on its own (environment or default); unknown keys are refused, as the agent
//! refuses them.

use std::collections::BTreeSet;

use kernel::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `collectors`: `MS_COLLECTORS_ALLOW` / `MS_COLLECTORS_DENY`. An empty `allow` runs every
/// collector enabled by default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectorSelection {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deny: Vec<String>,
}

/// `profiling`: continuous native profiling, `MS_PROFILING_*`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContinuousProfiling {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oncpu: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offcpu: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_hz: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_workloads: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offcpu_min_duration_us: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collectors: Option<CollectorSelection>,
    /// `MS_DROPWATCH_FILTER`: a tcpdump-subset filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dropwatch_filter: Option<String>,
    /// `MS_MEMORY_RECLAIM_BLOCKED_THRESHOLD_NS`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_reclaim_blocked_threshold_ns: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profiling: Option<ContinuousProfiling>,
}

fn within(name: &str, value: Option<u64>, range: std::ops::RangeInclusive<u64>) -> Result<()> {
    match value {
        Some(value) if !range.contains(&value) => Err(Error::invalid(format!(
            "`{name}` must be between {} and {}",
            range.start(),
            range.end()
        ))),
        _ => Ok(()),
    }
}

fn collector_name_valid(name: &str) -> bool {
    let mut chars = name.chars();

    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && name.len() <= 64
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

impl NodeConfig {
    /// Reads and checks a configuration document; the stored form is [`Self::to_value`].
    pub fn parse(value: Value) -> Result<Self> {
        // serde also reads a struct from an array; a configuration is an object.
        if !value.is_object() {
            return Err(Error::invalid("a node configuration must be a JSON object"));
        }

        let config: Self = serde_json::from_value(value)
            .map_err(|error| Error::invalid(format!("invalid node configuration: {error}")))?;

        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if let Some(collectors) = &self.collectors {
            let names = collectors.allow.iter().chain(&collectors.deny);

            if let Some(bad) = names.clone().find(|name| !collector_name_valid(name)) {
                return Err(Error::invalid(format!("`{bad}` is not a collector name")));
            }

            if collectors.allow.len() + collectors.deny.len() > 256 {
                return Err(Error::invalid("at most 256 collector names"));
            }

            let allowed: BTreeSet<_> = collectors.allow.iter().collect();

            if let Some(both) = collectors.deny.iter().find(|name| allowed.contains(name)) {
                return Err(Error::invalid(format!(
                    "`{both}` cannot be both allowed and denied"
                )));
            }
        }

        if self
            .dropwatch_filter
            .as_ref()
            .is_some_and(|filter| filter.len() > 1024)
        {
            return Err(Error::invalid(
                "`dropwatch_filter` must stay within 1024 characters",
            ));
        }

        within(
            "memory_reclaim_blocked_threshold_ns",
            self.memory_reclaim_blocked_threshold_ns,
            1_000_000..=60_000_000_000,
        )?;

        if let Some(profiling) = &self.profiling {
            within("profiling.interval_secs", profiling.interval_secs, 1..=3600)?;
            within("profiling.frequency_hz", profiling.frequency_hz, 1..=1000)?;
            within("profiling.max_workloads", profiling.max_workloads, 1..=1024)?;
            within(
                "profiling.offcpu_min_duration_us",
                profiling.offcpu_min_duration_us,
                0..=10_000_000,
            )?;
        }

        Ok(())
    }

    /// The document as stored and sent: only the keys that are set.
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| Value::Object(Default::default()))
    }
}
