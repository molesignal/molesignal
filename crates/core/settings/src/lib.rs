// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Typed deployment and runtime settings.

pub mod shared {
    pub use kernel::{Error, Result, ids};
    pub use signals::{self_telemetry, tail_sampling};
}

pub mod config;

pub use config::*;
