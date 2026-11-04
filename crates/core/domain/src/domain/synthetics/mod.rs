// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! User-facing synthetic monitoring domain.
//!
//! The domain owns stable resource identity, immutable monitor revisions, Probe locations,
//! execution results, and repository ports. Scheduling, transport, persistence, and protocol
//! conversion remain in the app/infra/api layers.

mod location;
pub mod monitor;
mod register;
mod repository;
mod result;
mod secret;
mod state;
mod task;

pub use location::{
    AgentCapacity, AgentStatus, EgressPolicy, LocationExecution, LocationHealth, LocationLifecycle,
    LocationScope, ProbeAgent, ProbeCapability, ProbeLocation,
};
pub use monitor::{
    ActiveMonitorRevision, AssertionOperator, AssertionSeverity, BrowserAction, BrowserJourneySpec,
    BrowserStep, ClientCertificate, ClientKey, DnsSpec, Extraction, GrpcCall, GrpcSpec,
    HeaderValue, HostBinding, HttpJourneySpec, HttpStep, HttpVersion, IcmpSpec, MonitorAssertion,
    MonitorKind, MonitorLifecycle, MonitorRevision, MonitorSchedule, MonitorSpec, MonitorState,
    MultiLocationPolicy, SshAuthentication, SshSpec, SyntheticMonitor, TcpSpec, TlsSpec,
    ValueSource, Viewport, assertion_source_allowed,
};
pub use register::{
    ProbeAgentToken, ProbeAgentTokenInstructions, ProbeAgentTokenStatus, ProbeRegisterInstructions,
    ProbeRegisterToken, ProbeRegistrationGrant,
};
pub use repository::{
    SyntheticAgentRepository, SyntheticAgentTokenRepository, SyntheticLocationRepository,
    SyntheticMonitorRepository, SyntheticRegisterRepository, SyntheticRepository,
    SyntheticResultRepository, SyntheticSecretRepository, SyntheticStateRepository,
    SyntheticTaskRepository,
};
pub use result::{
    AssertionObservation, ProbeAttempt, ProbeOutcome, ProbeStepEvidence, PurgedResults,
    SyntheticResult, SyntheticResultArtifact, SyntheticResultListQuery, SyntheticResultPage,
    TimingBreakdown,
};
pub use secret::{SecretMaterial, SyntheticSecret, SyntheticSecretVersion};
pub use state::{
    LocationStateUpdate, MissedRun, MonitorLocationState, StateObservation,
    SyntheticStateTransition,
};
pub use task::{ProbeTask, ProbeTaskState};
