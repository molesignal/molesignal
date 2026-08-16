// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! RUM physical read models, following the same write-time projection and direct Parquet scan
//! pattern as trace summaries. These readers never invoke DataFusion.

mod arrays;
mod model;
mod scan;
mod sessions;

pub use model::{RumActionRecord, RumErrorRecord, RumScope, RumSessionRecord};
pub use scan::{RumReadModelReader, ScanStats};
pub use sessions::{SessionPageBoundary, SessionPageQuery};
