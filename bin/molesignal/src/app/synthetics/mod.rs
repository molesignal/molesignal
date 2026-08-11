// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Synthetic monitoring use cases and deterministic scheduling/state decisions.

mod aggregation;
mod dispatch;
mod model;
mod probe;
mod runtime;
mod schedule;
mod service;
mod validation;

pub use aggregation::aggregate_locations;
pub use model::{
    CreateLocationInput, CreateMonitorInput, CreateSecretInput, UpdateAgentConfigurationInput,
};
pub use probe::{
    PROBE_PROTOCOL_VERSION, ProbeControlService, ProbeRegisterInput, ProbeRegisterOutcome,
    ResolvedProbeSecret,
};
pub use runtime::{ProcessResultOutcome, SyntheticTransitionSink};
pub use schedule::next_due_at;
pub use service::SyntheticService;
pub use validation::validate_revision;
