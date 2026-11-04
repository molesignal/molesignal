// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::field::{RumField, RumUnit};

/// What an `action` event adds to the common columns (`action.id` is common: on the other events
/// it names the action they happened in).
pub(super) const ACTION: &[RumField] = &[
    RumField::text("action.type"),
    RumField::ns("action.loading_time"),
    RumField::text("action.target.name").long(512),
    RumField::text("action.target.selector").long(1024),
    RumField::text("action.target.path").long(2048),
    RumField::int("action.target.width").unit(RumUnit::Pixels),
    RumField::int("action.target.height").unit(RumUnit::Pixels),
    RumField::int("action.target.offset.x").unit(RumUnit::Pixels),
    RumField::int("action.target.offset.y").unit(RumUnit::Pixels),
    RumField::int("action.position.x").unit(RumUnit::Pixels),
    RumField::int("action.position.y").unit(RumUnit::Pixels),
    RumField::int("action.page_position.x").unit(RumUnit::Pixels),
    RumField::int("action.page_position.y").unit(RumUnit::Pixels),
    RumField::text("action.name_source"),
    RumField::json("action.frustration.type"),
    RumField::count("action.frustration.count"),
    RumField::count("action.error.count"),
    RumField::count("action.crash.count"),
    RumField::count("action.long_task.count"),
    RumField::count("action.resource.count"),
];
