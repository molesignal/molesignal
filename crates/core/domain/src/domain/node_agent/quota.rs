// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use kernel::{Error, Result};
use serde::{Deserialize, Serialize};

/// How many node tasks an organization may have: at once (queued, offered or running, per
/// organization and per node) and per hour (created in the last hour). A task that ends frees its
/// slot at once; the hourly count frees itself as the hour moves on. Stored as the organization
/// setting `node_agent.task_quota`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskQuota {
    pub max_active_per_org: u32,
    pub max_active_per_node: u32,
    pub max_created_per_hour_per_org: u32,
    pub max_created_per_hour_per_node: u32,
}

impl Default for TaskQuota {
    fn default() -> Self {
        Self {
            max_active_per_org: 50,
            max_active_per_node: 4,
            max_created_per_hour_per_org: 600,
            max_created_per_hour_per_node: 60,
        }
    }
}

/// What an organization and a node use now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QuotaUsage {
    pub org_active: u32,
    pub node_active: u32,
    pub org_created_last_hour: u32,
    pub node_created_last_hour: u32,
}

impl TaskQuota {
    /// The window the hourly limits count over.
    pub const WINDOW_MICROS: i64 = 3600 * 1_000_000;
    const CEILING: u32 = 100_000;

    pub fn validate(&self) -> Result<()> {
        let limits = [
            ("maxActivePerOrg", self.max_active_per_org),
            ("maxActivePerNode", self.max_active_per_node),
            ("maxCreatedPerHourPerOrg", self.max_created_per_hour_per_org),
            (
                "maxCreatedPerHourPerNode",
                self.max_created_per_hour_per_node,
            ),
        ];

        for (name, value) in limits {
            if value == 0 || value > Self::CEILING {
                return Err(Error::invalid(format!(
                    "`{name}` must be between 1 and {}",
                    Self::CEILING
                )));
            }
        }

        if self.max_active_per_node > self.max_active_per_org {
            return Err(Error::invalid(
                "`maxActivePerNode` cannot exceed `maxActivePerOrg`",
            ));
        }

        Ok(())
    }

    /// Whether one more task fits; the error names the limit it would exceed.
    pub fn admit(&self, usage: QuotaUsage) -> Result<()> {
        let checks = [
            (
                usage.org_active >= self.max_active_per_org,
                "the organization already has the most active node tasks it may have",
            ),
            (
                usage.node_active >= self.max_active_per_node,
                "the node already has the most active tasks it may have",
            ),
            (
                usage.org_created_last_hour >= self.max_created_per_hour_per_org,
                "the organization created the most node tasks it may create in an hour",
            ),
            (
                usage.node_created_last_hour >= self.max_created_per_hour_per_node,
                "the node was given the most tasks it may get in an hour",
            ),
        ];

        match checks.into_iter().find(|(exceeded, _)| *exceeded) {
            Some((_, message)) => Err(Error::resource_exhausted(message)),
            None => Ok(()),
        }
    }

    /// How many more tasks the node may have offered or running, given those it has.
    pub fn node_slots(&self, max_running_tasks: u32, in_flight: u32) -> u32 {
        self.max_active_per_node
            .min(max_running_tasks.max(1))
            .saturating_sub(in_flight)
    }
}
