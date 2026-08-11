// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! ACME client + helpers（change `domain-acme-tls`，仅  feature）。
//!
//! - [`client::AcmeClient`]：`instant-acme` 包装；`ensure_account` + `issue`。
//! - 同目录 cfg-gated 编译，OSS build 时整个 module tree 不存在。

pub mod client;

pub use client::{AcmeClient, IssuedCert};
