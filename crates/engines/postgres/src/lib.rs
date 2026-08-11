// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! PostgreSQL repositories, migrations, encrypted persistence, and Probe CA storage.

pub mod agent {
    pub use ::agent::*;
}

pub mod config {
    pub use settings::*;
}

pub mod domain {
    pub use ::domain::*;
    pub use signals::policy as trace_policy;
}

pub mod shared {
    pub mod contracts {
        pub use ::contracts::*;
    }

    pub use kernel::{Error, Result, ids, time};
    pub use signals::{metrics, tail_sampling, trace_context, trace_normalization};
}

pub mod cipher;
pub mod persistence;
pub mod synthetics;

pub mod infra {
    pub mod cipher {
        pub use crate::cipher::*;
    }

    pub mod persistence {
        pub use crate::persistence::*;
    }

    pub mod runtime {
        pub use function_runtime::*;
    }
}

pub use persistence::{MetaStore, pool, repositories, sqlx_err};
