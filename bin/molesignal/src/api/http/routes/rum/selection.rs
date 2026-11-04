// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What the explorer is looking at: a kind of row, a range of time, filters and words. The list,
//! the histogram and the field values of the explorer all select by exactly this, so the bars
//! count the rows the list pages through. The tools that read what the pages read select the same
//! way, so it lives in the app layer.

pub(super) use crate::app::rum::selection::{Selection, stream_of};
