// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Microsoft Azure: pulling Azure Monitor metrics ([`monitor`]) and reading what Azure Monitor
//! exports to an event hub ([`event_hub_records`]).

pub mod event_hub_records;
pub mod monitor;
