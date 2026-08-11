// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use super::MonitorState;
use crate::shared::{ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorLocationState {
    pub organization_id: Id,
    pub monitor_id: Id,
    pub monitor_revision_id: Id,
    pub location_id: Id,
    pub current_state: MonitorState,
    pub candidate_state: Option<MonitorState>,
    pub candidate_count: u32,
    pub last_result_id: Option<Id>,
    pub last_observed_at: Option<TimestampMicros>,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntheticStateTransition {
    pub id: Id,
    pub organization_id: Id,
    pub monitor_id: Id,
    pub monitor_revision_id: Id,
    pub location_id: Option<Id>,
    pub previous_state: MonitorState,
    pub current_state: MonitorState,
    pub correlation_key: String,
    pub result_id: Option<Id>,
    pub started_at: TimestampMicros,
    pub ended_at: Option<TimestampMicros>,
    pub created_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateObservation {
    pub organization_id: Id,
    pub monitor_id: Id,
    pub monitor_revision_id: Id,
    pub location_id: Id,
    pub observed_state: MonitorState,
    pub result_id: Id,
    pub observed_at: TimestampMicros,
    pub failure_threshold: u32,
    pub recovery_threshold: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationStateUpdate {
    pub state: MonitorLocationState,
    pub transition: Option<SyntheticStateTransition>,
}
