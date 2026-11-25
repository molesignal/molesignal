// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Dashboards the product installs for an organization and keeps current: read-only, copied
//! before they are changed. The models are written here, in code, so that the SQL of a panel is
//! checked with the rest of the code.

pub mod host;
mod install;
mod panel;
pub mod rum;
pub mod sample;

pub use host::HostDashboard;
pub use install::{Builtin, Installed, Standing, installed_version};
pub use rum::RumDashboard;

#[cfg(test)]
mod tests;
