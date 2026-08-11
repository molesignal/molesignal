// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Alerting infra adapters。
//!
//! - [`realtime`]：Real-time alert matcher cache
//!
//! anomaly detector（MAD baseline）已上移到
//! `crate::domain::alerting::anomaly`，好让 app 层 evaluator 直接按
//! `AlertRule.kind = Anomaly` 分发（app 不依赖 infra）。

pub mod realtime;
