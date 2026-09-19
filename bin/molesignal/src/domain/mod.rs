// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Compatibility facade while the server package is split into workspace crates.

pub use ::domain::*;
pub use signals::policy as trace_policy;
