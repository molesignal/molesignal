// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The boundary of Mole Agent backend capabilities.
//!
//! `Mole Agent` is the product module name and also refers to the agent that interacts with users
//! and conducts investigations. All model calls, tool registration, investigation, approval, and
//! execution capabilities are exported from this module to avoid multiple naming schemes.

pub mod answer;
pub mod automations;
pub mod capabilities;
pub mod chat;
pub mod commands;
pub mod currencies;
pub mod feedback;
pub mod im;
pub mod inbound_mcp;
pub mod insights;
pub mod knowledge;
pub mod mcp;
pub mod memory;
pub mod model;
pub mod network;
pub mod references;
pub mod runs;
pub mod scope;
pub mod scoring;
pub mod skills;
pub mod telemetry;
pub mod titles;
pub mod tool_control;
pub mod tools;
pub mod usage;

pub const PRODUCT_NAME: &str = "Mole Agent";
pub const AGENT_NAME: &str = "Mole Agent";
