// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Numbers over many rows: how many, how many different, sums, averages, extremes and percentiles
//! of one stream, for each group of up to two columns and, when asked, for each bar of time.
//!
//! A [`Plan`] checks every name it is given against the fixed columns of the stream, so nothing a
//! client sends reaches a statement but as a quoted column of that stream, a number the plan
//! parsed itself, or a literal. The overview and the explorer's charts, tables and maps all read
//! through it.

mod answer;
mod measure;
mod panel;
mod plan;

pub use self::{
    answer::Aggregate,
    measure::MeasureSpec,
    plan::{Plan, Spec},
};
