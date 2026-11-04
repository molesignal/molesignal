// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::field::{RumField, RumUnit};

/// What an `error` event adds to the common columns.
pub(super) const ERROR: &[RumField] = &[
    RumField::text("error.id").exact(),
    RumField::text("error.message").full_text().long(8192),
    RumField::text("error.source"),
    RumField::text("error.type").long(256),
    RumField::text("error.stack").long(32768),
    RumField::json("error.frames"),
    // What the SDK names when the developer wants errors grouped their own way; the server's
    // `error.issue_id` is what groups them.
    RumField::text("error.fingerprint").exact().long(128),
    RumField::text("error.issue_id").exact().long(64),
    RumField::text("error.handling"),
    RumField::text("error.handling_stack").long(32768),
    RumField::json("error.causes"),
    RumField::flag("error.is_crash"),
    RumField::text("error.category"),
    RumField::text("error.source_type"),
    RumField::text("error.resource.method"),
    RumField::int("error.resource.status_code"),
    RumField::text("error.resource.url").long(4096),
    RumField::json("error.threads"),
    RumField::json("error.binary_images"),
    RumField::flag("error.was_truncated"),
    RumField::text("error.csp.disposition"),
    RumField::int("error.time_since_app_start").unit(RumUnit::Milliseconds),
    RumField::json("error.meta"),
    RumField::json("error.symbolication"),
];
