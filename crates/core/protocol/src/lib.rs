// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! MoleSignal protobuf and gRPC bindings.
//!
//! Cargo generates the Rust bindings from `proto/**/*.proto` into `OUT_DIR`; generated files are
//! build artifacts and are never stored in the source tree or committed to Git.

pub mod cluster {
    #[allow(clippy::all)]
    #[rustfmt::skip]
    pub mod v1 {
        tonic::include_proto!("cluster.v1");
    }
}

pub mod intake {
    #[allow(clippy::all)]
    #[rustfmt::skip]
    pub mod v1 {
        tonic::include_proto!("intake.v1");
    }
}

pub mod query {
    #[allow(clippy::all)]
    #[rustfmt::skip]
    pub mod v1 {
        tonic::include_proto!("query.v1");
    }
}

pub mod probe {
    #[allow(clippy::all)]
    #[rustfmt::skip]
    pub mod v1 {
        tonic::include_proto!("probe.v1");
    }
}

/// Vendored, version-pinned pprof profile format (`perftools.profiles`).
///
/// Source proto: `proto/pprof/v1/profile.proto`. This is the canonical wire form for continuous
/// profiling; protocol adapters normalize through these semantics.
pub mod pprof {
    #[allow(clippy::all)]
    #[rustfmt::skip]
    pub mod profiles {
        tonic::include_proto!("perftools.profiles");
    }
}
