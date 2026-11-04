// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What each source has taken in, and how it stands against its budget.
//!
//! Requests are measured where they arrive, on the node that answers them, because only there the
//! credential is known for every signal: a batch forwarded to an intake node carries none, and a
//! span in the tail-sampling queue neither. The measure is what was accepted, so that a request
//! the sender retries counts once. It is kept in memory and written in batches, which leaves
//! intake with an addition under a short lock and no database write.

mod budgets;
mod meter;
mod sources;

pub use budgets::{BudgetStanding, budget_standings};
pub use meter::{
    IntakeReceipt, IntakeUsageMeter, ServiceTally, service_of_fields, split_by_events,
};
pub use sources::{IntakeSourceDirectory, SourceLabel};
