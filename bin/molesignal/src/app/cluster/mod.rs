// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Compatibility façade for the cluster discovery domain port.

pub mod gate;
pub mod ownership;
pub mod registry;

pub use gate::{ShardedRunGate, record_overlap_skipped};
pub use ownership::{OwnershipView, PRIMARY_KEY, WorkOwnership};

pub use crate::domain::cluster::*;
