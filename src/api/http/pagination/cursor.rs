// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Shared cursor protocol for high-volume, continuously changing datasets.
//!
//! Signal-specific modules own their sort tuple and keyset predicate. This
//! module owns the signed envelope, page-size-plus-one trimming, direction
//! semantics, and the stable HTTP response shape.

use serde::{Serialize, de::DeserializeOwned};

pub use crate::shared::cursor::{CursorDirection, CursorPage, TrimmedCursorPage, trim_cursor_page};
use crate::{
    app::iam::IamService,
    shared::{Result, ids::Id},
};

pub const DEFAULT_CURSOR_TTL_SECS: u64 = 24 * 60 * 60;

pub fn encode_signed_cursor<T>(
    iam: &IamService,
    org_id: &Id,
    purpose: &str,
    payload: T,
) -> Result<String>
where
    T: Clone + Serialize,
{
    iam.issue_scoped_token(purpose, org_id, payload, DEFAULT_CURSOR_TTL_SECS)
}

pub fn decode_signed_cursor<T>(
    iam: &IamService,
    org_id: &Id,
    purpose: &str,
    token: &str,
) -> Result<T>
where
    T: Clone + DeserializeOwned,
{
    iam.verify_scoped_token(purpose, org_id, token)
}
