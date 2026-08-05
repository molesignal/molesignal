// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! TLS server-side glue（change `domain-acme-tls`，仅  feature）。

pub mod sni_resolver;

pub use sni_resolver::SniCertResolver;
