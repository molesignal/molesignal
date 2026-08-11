// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! External adapters used by customer Status Pages.

mod admin_notifier;
mod assets;
mod dns_verifier;

pub use admin_notifier::StatusPageAdminEmailNotifier;
pub use assets::StatusPageAssetCleaner;
pub use dns_verifier::DnsStatusPageDomainVerifier;
