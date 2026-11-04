// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 合并来电：同一个连接器拨同一个号码是一条线路，一个窗口里只拨一次；其间的通知排在线路上，窗口结束
//! 时合并成一通电话。

use serde::{Deserialize, Serialize};

use super::connector::{NotifyMessage, NotifyTarget};
use crate::shared::{ids::Id, time::TimestampMicros};

/// 一条电话线路。`key` 是号码的摘要：线路记录里不存号码本身。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallLine {
    pub organization_id: Id,
    pub connector_id: Id,
    pub key: String,
    pub window_secs: u32,
}

/// 排在线路上、等着合并进下一通电话的一条通知。`target` 是要拨的号码，拨完就随这条记录删掉。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedCall {
    pub organization_id: Id,
    pub delivery_id: Id,
    pub connector_id: Id,
    pub line_key: String,
    pub target: NotifyTarget,
    pub message: NotifyMessage,
    pub created_at: TimestampMicros,
}
