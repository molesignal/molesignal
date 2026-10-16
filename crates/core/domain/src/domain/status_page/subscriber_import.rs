// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Email subscribers brought over from another status page.

use serde::{Deserialize, Serialize};

use super::SubscriberComponents;
use crate::shared::time::TimestampMicros;

/// What an imported address becomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusPageSubscriberImportMode {
    /// The addresses confirmed elsewhere: they are active at once and are sent
    /// nothing now.
    Active,
    /// Each address is sent a confirmation and stays pending until it confirms.
    Confirm,
}

#[derive(Debug, Clone)]
pub struct ImportedStatusPageSubscriber {
    /// A normalized email address.
    pub target: String,
    pub token: String,
    pub token_hash: String,
    pub components: SubscriberComponents,
}

#[derive(Debug, Clone)]
pub struct StatusPageSubscriberImport {
    pub mode: StatusPageSubscriberImportMode,
    pub subscribers: Vec<ImportedStatusPageSubscriber>,
    /// Counts what an import would do without changing anything.
    pub dry_run: bool,
    pub imported_at: TimestampMicros,
}

/// What an import did, or would do. An address that is already active is left
/// as it is, and one that unsubscribed from the page is not subscribed again.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusPageSubscriberImportOutcome {
    /// Addresses that became active, or pending awaiting confirmation.
    pub imported: u64,
    pub already_subscribed: u64,
    pub skipped_unsubscribed: u64,
    pub confirmations_queued: u64,
}
