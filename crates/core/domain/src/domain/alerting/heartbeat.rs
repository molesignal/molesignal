// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 心跳规则：没有查询，给定时任务、备份脚本这类东西一个打卡地址。一个周期加宽限过去还没打卡就触发，
//! 打卡时报了失败马上触发；再次正常打卡就恢复。地址里的 Token 只存哈希。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

pub const MIN_HEARTBEAT_PERIOD_SECS: u32 = 60;
pub const MAX_HEARTBEAT_PERIOD_SECS: u32 = 7 * 86_400;
pub const MAX_HEARTBEAT_GRACE_SECS: u32 = 86_400;

/// 打卡时带的话最多留多少字。
pub const MAX_HEARTBEAT_MESSAGE_CHARS: usize = 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeartbeatParams {
    /// 多久打一次卡。
    pub period_secs: u32,
    /// 过了周期再等多久才算没打卡。
    #[serde(default)]
    pub grace_secs: u32,
}

impl HeartbeatParams {
    pub fn validate(&self) -> Result<()> {
        if !(MIN_HEARTBEAT_PERIOD_SECS..=MAX_HEARTBEAT_PERIOD_SECS).contains(&self.period_secs) {
            return Err(Error::invalid(format!(
                "heartbeat.period_secs is {MIN_HEARTBEAT_PERIOD_SECS} to \
                 {MAX_HEARTBEAT_PERIOD_SECS}"
            )));
        }

        if self.grace_secs > MAX_HEARTBEAT_GRACE_SECS {
            return Err(Error::invalid(format!(
                "heartbeat.grace_secs is at most {MAX_HEARTBEAT_GRACE_SECS}"
            )));
        }

        Ok(())
    }

    fn allowance_micros(&self) -> i64 {
        (i64::from(self.period_secs) + i64::from(self.grace_secs)) * 1_000_000
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeartbeatStatus {
    #[default]
    Ok,
    Fail,
}

impl HeartbeatStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Fail => "fail",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "fail" => Self::Fail,
            _ => Self::Ok,
        }
    }
}

/// 一条心跳规则的打卡地址与最后一次打卡。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Heartbeat {
    pub rule_id: Id,
    pub org_id: Id,
    #[serde(skip_serializing)]
    pub token_hash: String,
    pub last_checkin_at: Option<TimestampMicros>,
    pub last_status: HeartbeatStatus,
    /// 最后一次打卡带的话（请求体的开头）。
    pub last_message: String,
    /// 地址发出（或重置）的时间：还没打过卡时从这里算起。
    pub created_at: TimestampMicros,
}

impl Heartbeat {
    /// 下一次最晚什么时候要打卡。
    pub fn due_at(&self, params: &HeartbeatParams) -> TimestampMicros {
        let since = self.last_checkin_at.unwrap_or(self.created_at);

        TimestampMicros(since.0.saturating_add(params.allowance_micros()))
    }

    /// 该不该响：最后一次打卡报了失败，或过了最晚的时间还没再打卡。
    pub fn alarming(&self, params: &HeartbeatParams, now: TimestampMicros) -> bool {
        self.last_status == HeartbeatStatus::Fail || now.0 > self.due_at(params).0
    }
}

#[async_trait]
pub trait HeartbeatRepository: Send + Sync {
    /// 发出或重置打卡地址：换了 Token 以后旧地址失效，打卡记录保留。
    async fn replace_token(
        &self,
        org_id: &Id,
        rule_id: &Id,
        token_hash: &str,
        now: TimestampMicros,
    ) -> Result<Heartbeat>;
    async fn get(&self, rule_id: &Id) -> Result<Option<Heartbeat>>;
    async fn find_by_token_hash(&self, token_hash: &str) -> Result<Option<Heartbeat>>;
    async fn record_checkin(
        &self,
        rule_id: &Id,
        at: TimestampMicros,
        status: HeartbeatStatus,
        message: &str,
    ) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: i64 = 60_000_000;

    fn heartbeat(last: Option<i64>, status: HeartbeatStatus) -> Heartbeat {
        Heartbeat {
            rule_id: Id::from_string("rule-1"),
            org_id: Id::from_string("org-1"),
            token_hash: String::new(),
            last_checkin_at: last.map(TimestampMicros),
            last_status: status,
            last_message: String::new(),
            created_at: TimestampMicros(0),
        }
    }

    #[test]
    fn a_missed_check_in_alarms_after_the_period_and_grace() {
        let params = HeartbeatParams {
            period_secs: 3600,
            grace_secs: 300,
        };
        let beat = heartbeat(Some(10 * MINUTE), HeartbeatStatus::Ok);

        assert_eq!(beat.due_at(&params), TimestampMicros(75 * MINUTE));
        assert!(!beat.alarming(&params, TimestampMicros(75 * MINUTE)));
        assert!(beat.alarming(&params, TimestampMicros(76 * MINUTE)));

        // 还没打过卡：从地址发出时算起。
        assert!(
            heartbeat(None, HeartbeatStatus::Ok).alarming(&params, TimestampMicros(66 * MINUTE))
        );
    }

    #[test]
    fn a_failed_check_in_alarms_at_once() {
        let params = HeartbeatParams {
            period_secs: 3600,
            grace_secs: 0,
        };

        assert!(
            heartbeat(Some(10 * MINUTE), HeartbeatStatus::Fail)
                .alarming(&params, TimestampMicros(11 * MINUTE))
        );
    }

    #[test]
    fn periods_have_bounds() {
        assert!(
            HeartbeatParams {
                period_secs: 30,
                grace_secs: 0
            }
            .validate()
            .is_err()
        );
        assert!(
            HeartbeatParams {
                period_secs: 3600,
                grace_secs: 2 * 86_400
            }
            .validate()
            .is_err()
        );
        assert!(
            HeartbeatParams {
                period_secs: 3600,
                grace_secs: 600
            }
            .validate()
            .is_ok()
        );
    }
}
