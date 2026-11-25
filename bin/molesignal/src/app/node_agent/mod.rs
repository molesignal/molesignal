// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What an organization gets for the hosts MoleSignal Node Agent watches, ready to use: the indexes
//! its host events are filtered by, saved queries of the OOM, network and hardware events, alert
//! rules of the events that need someone at once, and the host dashboard
//! ([`crate::app::dashboard::builtin::HostDashboard`]).
//!
//! The agent writes metrics into [`METRICS_STREAM`] (OTLP metrics named `molesignal_host_*`) and
//! kernel and hardware events into [`EVENTS_STREAM`] (OTLP logs: `event.name`, `event.domain`,
//! `event.kind` and `severity_text`, the host and the container in the resource, the event's
//! fields in `body`). Profiles go to the profile intake.

pub mod alerts;
pub mod indexes;
pub mod views;

/// The stream the agent writes its metrics into (`MS_STREAM_NAME`).
pub const METRICS_STREAM: &str = "host-metrics";

/// The stream the agent writes its events into (`MS_STREAM_EVENTS`).
pub const EVENTS_STREAM: &str = "host-events";

/// The label every alert rule and the tag every saved query made here carries.
pub const TAG: &str = "node-agent";

#[cfg(test)]
mod tests;
