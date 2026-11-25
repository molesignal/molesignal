// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Node Agents in managed mode: their own inventory, enrollment tokens, task leases and quotas.
//!
//! Nothing here is shared with Probe Agents: a node has its own identity (a certificate from the
//! Node Agent CA, bound to one organization), and its tasks are the closed set of [`TaskKind`]s.
//! Task transitions are pure ([`task`]), so the Postgres repository and the tests apply the same
//! rules.

mod config;
mod enrollment;
mod node;
mod quota;
mod repository;
pub mod spec;
pub mod task;

#[cfg(test)]
mod config_tests;
#[cfg(test)]
mod quota_tests;
#[cfg(test)]
mod spec_tests;
#[cfg(test)]
mod task_tests;

pub use config::{CollectorSelection, ContinuousProfiling, NodeConfig};
pub use enrollment::{EnrollmentGrant, EnrollmentToken, NewEnrollmentToken};
pub use node::{
    Capabilities, CollectorState, ConfigState, ConfigStatus, DesiredConfig, Health, HealthState,
    NodeAgent, NodeStatus, ProtocolVersion, SessionReport,
};
pub use quota::{QuotaUsage, TaskQuota};
pub use repository::{NodeAgentRepository, TaskFilter};
pub use task::{
    LeaseKey, NodeTask, RejectReason, ResultOutcome, TaskEvent, TaskKind, TaskOutcome, TaskState,
    TaskStatus, Transition,
};
