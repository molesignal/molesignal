// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Function 运行时：
//!
//! - [`vrl::runtime::VrlRuntime`]：编译 + 执行 VRL 源码，把 `serde_json::Value` 当作
//!   target，stdlib 函数（`parse_json` / `to_int` / `del` / `match` 等）默认可用。
//! - [`vrl::executor::VrlFunctionExecutor`]：实现 `app::ingestion::FunctionExecutor`，
//!   包装 `VrlRuntime` + per-function compile cache。
//! - [`chained_executor::ChainedFunctionExecutor`]：把 VRL + 可选 JS 组合成单一
//!   `FunctionExecutor`，按 `function.language` 路由。
//! - [`js_executor::JsFunctionExecutor`]（仅 `feature = "js-runtime"`）：基于
//!   `deno_core` 的 V8 isolate runtime，spec functions-runtime ADDED 段所述。

pub mod chained_executor;
pub mod vrl;

#[cfg(feature = "js-runtime")]
pub mod js_executor;

pub use chained_executor::ChainedFunctionExecutor;
#[cfg(feature = "js-runtime")]
pub use js_executor::JsFunctionExecutor;
pub use vrl::{VrlFunctionExecutor, VrlRuntime};
