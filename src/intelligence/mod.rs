// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Intelligence 后端能力边界。
//!
//! `Mole Intelligence` 是产品模块名，`Mole Agent` 是与用户交互并执行调查的智能体。
//! 所有模型调用、工具注册、调查、审批和执行能力都从本模块导出，避免再出现多套命名。

pub mod capabilities;
pub mod chat;
pub mod model;
pub mod telemetry;
pub mod tool_control;
pub mod tools;

pub const FEATURE: &str = "intelligence";
pub const PRODUCT_NAME: &str = "Mole Intelligence";
pub const AGENT_NAME: &str = "Mole Agent";
