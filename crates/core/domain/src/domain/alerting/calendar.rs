// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 值班日历的订阅：一个人订自己在所有排班里的班次，或订一个排班的全部班次。订阅地址里带一个随机
//! Token，只存它的哈希；丢了就重置，旧地址随即失效。

use async_trait::async_trait;
use serde::Serialize;

use crate::shared::{Result, ids::Id, time::TimestampMicros};

/// 订阅的日历从多久以前、到多久以后。
pub const CALENDAR_PAST_MICROS: i64 = 30 * 86_400_000_000;
pub const CALENDAR_FUTURE_MICROS: i64 = 90 * 86_400_000_000;

/// 一个人的一份订阅。`schedule_id` 为空时是他自己的班次。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarFeed {
    pub id: Id,
    pub org_id: Id,
    pub user_id: Id,
    pub schedule_id: Option<Id>,
    #[serde(skip_serializing)]
    pub token_hash: String,
    pub created_at: TimestampMicros,
    pub last_used_at: Option<TimestampMicros>,
}

#[async_trait]
pub trait CalendarFeedRepository: Send + Sync {
    /// 存下这份订阅；同一个人对同一个排班（或他自己）已有的那份被换掉。
    async fn replace(&self, feed: CalendarFeed) -> Result<CalendarFeed>;
    async fn list_for_user(&self, org_id: &Id, user_id: &Id) -> Result<Vec<CalendarFeed>>;
    async fn find_by_token_hash(&self, token_hash: &str) -> Result<Option<CalendarFeed>>;
    /// 删掉这个人的这份订阅，返回有没有删到。
    async fn delete(&self, org_id: &Id, user_id: &Id, id: &Id) -> Result<bool>;
    async fn touch(&self, id: &Id, at: TimestampMicros) -> Result<()>;
}
