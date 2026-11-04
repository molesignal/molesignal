// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Synthetic monitoring use cases and deterministic scheduling/state decisions.

mod agent_tokens;
mod aggregation;
mod artifacts;
pub mod availability;
mod dispatch;
mod model;
mod no_data;
mod probe;
mod result_stream;
mod runtime;
mod schedule;
mod service;
mod unclaimed;
mod validation;

pub use agent_tokens::{CreateAgentTokenInput, RotateAgentTokenInput};
pub use aggregation::aggregate_locations;
pub use artifacts::{
    MAX_HAR_BYTES, MAX_SCREENSHOT_BYTES, MAX_TRACE_BYTES, SyntheticArtifactTarget, artifact_expiry,
    artifact_object_key, artifact_targets,
};
pub use model::{
    CreateLocationInput, CreateMonitorInput, CreateSecretInput, UpdateAgentConfigurationInput,
};
pub use no_data::{MissingLocation, MissingRuns, NoDataChange};
pub use probe::{
    CLIENT_KEY_REFERENCE, PROBE_PROTOCOL_VERSION, ProbeControlService, ProbeRegisterInput,
    ProbeRegisterOutcome, ResolvedProbeSecret,
};
pub use result_stream::{
    RESULT_STREAM, ResultStreamQueue, ResultStreamReceiver, result_stream_queue, stream_fields,
};
pub use runtime::{ProcessResultOutcome, SyntheticTransitionSink, SyntheticTransitionSinks};
pub use schedule::next_due_at;
pub use service::SyntheticService;
pub use validation::validate_revision;
