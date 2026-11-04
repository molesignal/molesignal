// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The RUM figures alert rules watch.
//!
//! - [`groups`] says which numbers each figure is made of and works the figure out of them.
//! - [`scope`] says which rows a rule is about.
//! - [`samples`] finds the sessions an incident is told by.
//! - [`RumAlerts`] reads them: through the aggregate the pages read through, over the streams as
//!   they are written, with the reads of one scope and window shared for a short while.

mod groups;
mod samples;
mod scope;
mod source;

pub use self::source::RumAlerts;
