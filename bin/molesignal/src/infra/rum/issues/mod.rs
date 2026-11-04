// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Issues: the errors of an application that are alike, as one thing people can act on.
//!
//! - [`identity`] works out the id of the issue an error belongs to. It is a function of the error
//!   alone, so every node that receives events agrees.
//! - [`IssueTracker`] runs where the errors of an organization are written, and keeps the issues:
//!   it opens new ones, notes the ones that come back, moves a resolved one seen in a newer version
//!   to regressed, and counts what an application opens in an hour.
//! - [`VersionLedger`] keeps the versions of an application in the order they were first seen,
//!   which is what a newer version is.

pub mod identity;
mod ledger;
mod tracker;

pub use self::{ledger::VersionLedger, tracker::IssueTracker};
