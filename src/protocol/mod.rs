// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! MoleSignal 协议绑定（Rust）。
//!
//! 源 `.proto` 文件位于项目根 `/proto/<pkg>/v1/*.proto`，每个 package 形如
//! `cluster.v1` / `ingest.v1` / `query.v1`。Rust 代码通过 `make proto`
//! 手动生成到 `src/protocol/`，生成产物随源码提交。
//!
//! 手动生成方式与 `match-engine-fabric` 保持一致：
//!
//! ```bash
//! make proto                                  # 生成 Rust 绑定
//! buf lint                                    # 静态检查
//! buf breaking --against '.git#branch=main'   # 兼容性检查
//! ```

pub mod cluster {
    #[allow(clippy::all)]
    #[rustfmt::skip]
    pub mod v1 {
        include!("cluster/v1/cluster.v1.rs");
    }
}

pub mod ingest {
    #[allow(clippy::all)]
    #[rustfmt::skip]
    pub mod v1 {
        include!("ingest/v1/ingest.v1.rs");
    }
}

pub mod query {
    #[allow(clippy::all)]
    #[rustfmt::skip]
    pub mod v1 {
        include!("query/v1/query.v1.rs");
    }
}

/// Vendored, version-pinned pprof profile format (`perftools.profiles`).
///
/// Source proto: `proto/pprof/v1/profile.proto`. This is the canonical wire
/// form for continuous profiling; the OTLP-profiles adapter and the Pyroscope
/// folded/lines adapters all normalize through these semantics.
pub mod pprof {
    #[allow(clippy::all)]
    #[rustfmt::skip]
    pub mod profiles {
        include!("perftools/profiles/perftools.profiles.rs");
    }
}
