// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use super::{connector::NotifyTargetType, preference::NotifyCategory};
use crate::shared::{ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifyDefaultRoute {
    pub connector_id: Id,
    pub target_type: NotifyTargetType,
    pub target: String,
    pub order: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamNotifyDefault {
    pub id: Id,
    pub organization_id: Id,
    pub team_id: Id,
    pub category: NotifyCategory,
    pub routes: Vec<NotifyDefaultRoute>,
    pub enabled: bool,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizationNotifyDefault {
    pub id: Id,
    pub organization_id: Id,
    pub category: NotifyCategory,
    pub routes: Vec<NotifyDefaultRoute>,
    pub enabled: bool,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}
