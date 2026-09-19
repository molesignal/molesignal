// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Search 差异化能力：节点级准入控制（work group 并发槽位）等。

pub mod admission;

pub use admission::{AdmissionConfig, AdmissionController, AdmissionStats, SlotGuard};
