// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What the RUM pages read, shared with the tools and the references of Mole Agent that read the
//! same: the reader that runs the statements, what the explorer selects, the numbers of an
//! application, its error issues, the questions of product analytics, where people click and how
//! far they scroll, what a model is told about a session, and the statement of a dashboard panel
//! that draws what the explorer draws.

pub mod aggregate;
pub mod analytics;
pub mod heatmap;
pub mod issues;
pub mod overview;
pub mod panel;
pub mod reader;
pub mod selection;
pub mod summary;
pub mod vitals;
