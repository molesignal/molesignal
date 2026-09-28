// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Business models and ports shared by MoleSignal modules and adapters.

pub mod shared {
    pub mod contracts {
        pub use ::contracts::*;
    }

    pub use kernel::{Error, Probe, Result, cursor, drain, error, health, ids, time};
}

pub mod domain;

pub use domain::*;
