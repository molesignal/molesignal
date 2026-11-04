// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 个人的后续步骤：告警事件送到一个人之后，到了下一步的等待时间仍没人确认，就用下一步的通知方式再发。

use crate::shared::{ids::Id, time::TimestampMicros};

/// 排好的一个后续步骤。`delay_secs` 是这一步的等待时间，`due_at` 是从第一次通知这个人算起到期的时刻。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotifyFollowUp {
    pub organization_id: Id,
    pub event_id: String,
    pub policy_id: Id,
    pub recipient_user_id: Id,
    pub delay_secs: i32,
    pub due_at: TimestampMicros,
}

impl NotifyFollowUp {
    /// 第一次通知这个人的时刻：各步的等待时间都从这里算。
    pub fn started_at(&self) -> TimestampMicros {
        TimestampMicros(
            self.due_at
                .0
                .saturating_sub(i64::from(self.delay_secs) * 1_000_000),
        )
    }
}
