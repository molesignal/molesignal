// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Dependency-light primitives shared by every MoleSignal package.

pub mod contracts {
    pub use ::contracts::*;
}
pub mod cursor;
pub mod drain;
pub mod error;
pub mod health;
pub mod ids;
pub mod license;
pub mod time;

pub use error::{Error, Result};
pub use health::Probe;
pub use license::{CommunityLicense, LicenseGate, LicenseHolder};
