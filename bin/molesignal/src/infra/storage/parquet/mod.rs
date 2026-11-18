// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Parquet read and write adapters.

pub mod clustering;
pub mod partition;
pub mod reader;
pub(crate) mod stored_text;
pub mod writer;
