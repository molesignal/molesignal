// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Function runtime:
//!
//! - [`vrl::runtime::VrlRuntime`]: compiles and runs VRL source with a `serde_json::Value` as
//!   the target; stdlib functions (`parse_json` / `to_int` / `del` / `match`, etc.) are
//!   available by default.
//! - [`vrl::executor::VrlFunctionExecutor`]: implements `domain::function::FunctionExecutor`,
//!   wrapping `VrlRuntime` plus a per-function compile cache.
//! - [`chained_executor::ChainedFunctionExecutor`]: combines VRL and optional JS into a single
//!   `FunctionExecutor`, routing by `function.language`.
//! - [`js_executor::JsFunctionExecutor`] (only with `feature = "js-runtime"`): a V8 isolate
//!   runtime based on `deno_core`, as described in the ADDED section of spec functions-runtime.

pub mod chained_executor;
pub mod vrl;

#[cfg(feature = "js-runtime")]
pub mod js_executor;

pub use chained_executor::ChainedFunctionExecutor;
#[cfg(feature = "js-runtime")]
pub use js_executor::JsFunctionExecutor;
pub use vrl::{VrlFunctionExecutor, VrlRuntime};
