// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! VRL runtime and ingestion executor.

pub mod executor;
pub mod runtime;

pub use executor::VrlFunctionExecutor;
pub use runtime::VrlRuntime;
