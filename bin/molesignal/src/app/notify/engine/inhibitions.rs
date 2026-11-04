// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 告警抑制压住的通知：谁在压谁由升级派发判定、写在事件上（`crate::app::alerting::Inhibitions`），这里只
//! 读事件。压着的通知和静默的一样：抑制结束时事件还开着就照常发，已确认或已恢复就不再发。

use super::silences::Silences;
use crate::shared::{Error, ids::Id, time::TimestampMicros};

impl Silences {
    /// 这个告警事件的通知此刻被抑制压着。事件读不出来时不算：宁可多发，和读不出静默时一样。
    pub(super) async fn inhibited(&self, alert_id: &Id, now: TimestampMicros) -> bool {
        match self.incidents.get(alert_id).await {
            Ok(incident) => incident.handling.inhibition_holds(now),
            Err(Error::NotFound(_)) => false,
            Err(error) => {
                tracing::warn!(%alert_id, %error, "incident could not be read for the inhibition check; not holding the notification");
                false
            }
        }
    }
}
