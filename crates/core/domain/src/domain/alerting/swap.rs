// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 换班申请：值班的人请另一个人替他值一段班。对方接受就生成一条替班；排班设了要管理员批准的，对方
//! 接受后还要有排班管理权限的人批准。开始时间到了还没定下来的申请过期。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

/// 备注最多多少字。
pub const MAX_SWAP_NOTE_CHARS: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwapStatus {
    /// 等对方答复。
    Pending,
    /// 对方接受了，等管理员批准。
    AwaitingApproval,
    /// 生效了：替班已经加上。
    Accepted,
    Declined,
    Rejected,
    Cancelled,
    Expired,
}

impl SwapStatus {
    pub const ALL: [Self; 7] = [
        Self::Pending,
        Self::AwaitingApproval,
        Self::Accepted,
        Self::Declined,
        Self::Rejected,
        Self::Cancelled,
        Self::Expired,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Accepted => "accepted",
            Self::Declined => "declined",
            Self::Rejected => "rejected",
            Self::Cancelled => "cancelled",
            Self::Expired => "expired",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|status| status.as_str() == value)
            .ok_or_else(|| Error::internal(format!("unknown swap status: {value}")))
    }

    /// 还没定下来：还能接受、拒绝、批准或撤回。
    pub const fn is_open(self) -> bool {
        matches!(self, Self::Pending | Self::AwaitingApproval)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwapRequest {
    pub id: Id,
    pub org_id: Id,
    pub schedule_id: Id,
    /// 发起的人：原本值这段班的。
    pub requester_id: Id,
    /// 替他值班的人。
    pub target_id: Id,
    pub start_at: TimestampMicros,
    pub end_at: TimestampMicros,
    pub note: String,
    pub status: SwapStatus,
    /// 生效后加上的替班。
    pub override_id: Option<Id>,
    pub decided_by: Option<Id>,
    pub decided_at: Option<TimestampMicros>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

impl SwapRequest {
    /// 新申请的检查：时间段在将来、两个人不同、备注不太长。
    pub fn validate(&self, now: TimestampMicros) -> Result<()> {
        if self.end_at.0 <= self.start_at.0 {
            return Err(Error::invalid("a swap must end after it starts"));
        }

        if self.end_at.0 <= now.0 {
            return Err(Error::invalid("a swap must still be ahead"));
        }

        if self.requester_id == self.target_id {
            return Err(Error::invalid("ask someone else to take the shift"));
        }

        if self.note.chars().count() > MAX_SWAP_NOTE_CHARS {
            return Err(Error::invalid(format!(
                "a swap note is at most {MAX_SWAP_NOTE_CHARS} characters"
            )));
        }

        Ok(())
    }
}

#[async_trait]
pub trait SwapRequestRepository: Send + Sync {
    async fn create(&self, request: SwapRequest) -> Result<SwapRequest>;
    async fn get(&self, org_id: &Id, id: &Id) -> Result<Option<SwapRequest>>;
    /// 只在状态还是 `expected` 时写入，返回有没有写进去。
    async fn update_if_status(&self, request: SwapRequest, expected: SwapStatus) -> Result<bool>;
    /// 一个排班的申请，新的在前。
    async fn list_for_schedule(&self, org_id: &Id, schedule_id: &Id) -> Result<Vec<SwapRequest>>;
    /// 一个人发起或被请的申请，新的在前。
    async fn list_for_user(&self, org_id: &Id, user_id: &Id) -> Result<Vec<SwapRequest>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(start: i64, end: i64) -> SwapRequest {
        SwapRequest {
            id: Id::new(),
            org_id: Id::from_string("org-a"),
            schedule_id: Id::from_string("sch-1"),
            requester_id: Id::from_string("u1"),
            target_id: Id::from_string("u2"),
            start_at: TimestampMicros(start),
            end_at: TimestampMicros(end),
            note: String::new(),
            status: SwapStatus::Pending,
            override_id: None,
            decided_by: None,
            decided_at: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    #[test]
    fn a_swap_must_be_ahead_between_two_people() {
        assert!(request(100, 200).validate(TimestampMicros(50)).is_ok());
        assert!(request(200, 100).validate(TimestampMicros(50)).is_err());
        assert!(request(100, 200).validate(TimestampMicros(300)).is_err());

        let mut same = request(100, 200);
        same.target_id = same.requester_id.clone();
        assert!(same.validate(TimestampMicros(50)).is_err());
    }

    #[test]
    fn every_status_reads_back_from_its_word() {
        for status in SwapStatus::ALL {
            assert_eq!(SwapStatus::parse(status.as_str()).expect("known"), status);
        }

        assert!(SwapStatus::Pending.is_open());
        assert!(!SwapStatus::Accepted.is_open());
    }
}
