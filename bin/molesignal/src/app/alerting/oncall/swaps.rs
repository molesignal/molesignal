// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 换班申请：值班的人请排班里的另一个人替他值一段班。对方接受就在排班最前面加一条替班；排班设了要
//! 批准的，还要有排班管理权限的人批准。每一步发一条 `oncall.swap.updated` 通知给该知道的人。

use std::sync::Arc;

use crate::{
    app::notify::{NotifyEngine, swap_dispatch},
    domain::alerting::{
        repositories::ScheduleRepository,
        schedule::{Schedule, ScheduleOverride},
        swap::{SwapRequest, SwapRequestRepository, SwapStatus},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 没写备注时替班的说明。
const SWAP_REASON: &str = "Shift swap";

/// 新申请填的内容；发起的人是调用的人。
pub struct NewSwap {
    pub target_id: Id,
    pub start_at: TimestampMicros,
    pub end_at: TimestampMicros,
    pub note: String,
}

/// 一步之后申请的样子；生效时还有加了替班的排班。
pub struct SwapOutcome {
    pub request: SwapRequest,
    pub schedule: Option<Schedule>,
}

pub struct ShiftSwaps {
    swaps: Arc<dyn SwapRequestRepository>,
    schedules: Arc<dyn ScheduleRepository>,
    notify: Arc<NotifyEngine>,
}

impl ShiftSwaps {
    pub fn new(
        swaps: Arc<dyn SwapRequestRepository>,
        schedules: Arc<dyn ScheduleRepository>,
        notify: Arc<NotifyEngine>,
    ) -> Self {
        Self {
            swaps,
            schedules,
            notify,
        }
    }

    /// 发起：这段时间里排班上值班的只能是发起的人，接班的是排班成员，同一段时间不能有两份没定的申请。
    pub async fn request(
        &self,
        schedule: &Schedule,
        requester_id: &Id,
        swap: NewSwap,
    ) -> Result<SwapRequest> {
        let now = TimestampMicros::now();

        let request = SwapRequest {
            id: Id::new(),
            org_id: schedule.org_id.clone(),
            schedule_id: schedule.id.clone(),
            requester_id: requester_id.clone(),
            target_id: swap.target_id,
            start_at: swap.start_at,
            end_at: swap.end_at,
            note: swap.note.trim().to_string(),
            status: SwapStatus::Pending,
            override_id: None,
            decided_by: None,
            decided_at: None,
            created_at: now,
            updated_at: now,
        };

        request.validate(now)?;

        if !is_member(schedule, &request.target_id) {
            return Err(Error::invalid(
                "ask a member of this schedule to take the shift",
            ));
        }

        holds_window(schedule, &request)?;

        let overlapping = self
            .swaps
            .list_for_schedule(&schedule.org_id, &schedule.id)
            .await?
            .into_iter()
            .any(|other| {
                other.status.is_open()
                    && other.end_at.0 > now.0
                    && other.requester_id == request.requester_id
                    && other.start_at.0 < request.end_at.0
                    && request.start_at.0 < other.end_at.0
            });

        if overlapping {
            return Err(Error::invalid(
                "an open swap already covers part of this window",
            ));
        }

        let created = self.swaps.create(request).await?;

        self.announce(schedule, &created, vec![created.target_id.clone()])
            .await;

        Ok(created)
    }

    /// 被请的人答复。接受时排班不用批准就马上生效，要批准就等管理员。
    pub async fn respond(
        &self,
        schedule: &Schedule,
        id: &Id,
        user_id: &Id,
        accept: bool,
    ) -> Result<SwapOutcome> {
        let request = self.open_request(schedule, id).await?;

        if &request.target_id != user_id {
            return Err(Error::forbidden(
                "only the person asked can answer this swap",
            ));
        }

        if request.status != SwapStatus::Pending {
            return Err(Error::conflict("this swap is not waiting for an answer"));
        }

        let now = TimestampMicros::now();

        if !accept {
            let declined = self
                .settle(
                    request,
                    SwapStatus::Pending,
                    SwapStatus::Declined,
                    user_id,
                    now,
                )
                .await?;

            self.announce(schedule, &declined, vec![declined.requester_id.clone()])
                .await;

            return Ok(SwapOutcome {
                request: declined,
                schedule: None,
            });
        }

        if schedule.swap_needs_approval {
            holds_window(schedule, &request)?;

            let waiting = SwapRequest {
                status: SwapStatus::AwaitingApproval,
                updated_at: now,
                ..request
            };

            if !self
                .swaps
                .update_if_status(waiting.clone(), SwapStatus::Pending)
                .await?
            {
                return Err(Error::conflict("this swap changed meanwhile"));
            }

            let mut recipients = managers(schedule);
            push_unique(&mut recipients, &waiting.requester_id);

            self.announce(schedule, &waiting, recipients).await;

            return Ok(SwapOutcome {
                request: waiting,
                schedule: None,
            });
        }

        self.apply(schedule, request, SwapStatus::Pending, user_id, now)
            .await
    }

    /// 管理员批准或驳回一份对方已经接受的申请。调用方先查过排班管理权限。
    pub async fn decide(
        &self,
        schedule: &Schedule,
        id: &Id,
        user_id: &Id,
        approve: bool,
    ) -> Result<SwapOutcome> {
        let request = self.open_request(schedule, id).await?;

        if request.status != SwapStatus::AwaitingApproval {
            return Err(Error::conflict("this swap is not waiting for approval"));
        }

        let now = TimestampMicros::now();

        if approve {
            return self
                .apply(
                    schedule,
                    request,
                    SwapStatus::AwaitingApproval,
                    user_id,
                    now,
                )
                .await;
        }

        let rejected = self
            .settle(
                request,
                SwapStatus::AwaitingApproval,
                SwapStatus::Rejected,
                user_id,
                now,
            )
            .await?;

        self.announce(
            schedule,
            &rejected,
            vec![rejected.requester_id.clone(), rejected.target_id.clone()],
        )
        .await;

        Ok(SwapOutcome {
            request: rejected,
            schedule: None,
        })
    }

    /// 发起的人在定下来之前撤回。
    pub async fn cancel(&self, schedule: &Schedule, id: &Id, user_id: &Id) -> Result<SwapRequest> {
        let request = self.open_request(schedule, id).await?;

        if &request.requester_id != user_id {
            return Err(Error::forbidden(
                "only the person who asked can withdraw this swap",
            ));
        }

        let expected = request.status;
        let cancelled = self
            .settle(
                request,
                expected,
                SwapStatus::Cancelled,
                user_id,
                TimestampMicros::now(),
            )
            .await?;

        self.announce(schedule, &cancelled, vec![cancelled.target_id.clone()])
            .await;

        Ok(cancelled)
    }

    /// 一个排班的申请，新的在前；时间段已经过去还没定的记成过期。
    pub async fn for_schedule(&self, schedule: &Schedule) -> Result<Vec<SwapRequest>> {
        let requests = self
            .swaps
            .list_for_schedule(&schedule.org_id, &schedule.id)
            .await?;

        Ok(self.expire_ended(requests).await)
    }

    /// 一个人发起或被请的申请。
    pub async fn for_user(&self, org_id: &Id, user_id: &Id) -> Result<Vec<SwapRequest>> {
        let requests = self.swaps.list_for_user(org_id, user_id).await?;

        Ok(self.expire_ended(requests).await)
    }

    /// 读出一份还没定的申请：属于这个排班；时间段已经过去的记成过期并报错。
    async fn open_request(&self, schedule: &Schedule, id: &Id) -> Result<SwapRequest> {
        let request = self
            .swaps
            .get(&schedule.org_id, id)
            .await?
            .filter(|request| request.schedule_id == schedule.id)
            .ok_or_else(|| Error::not_found("swap not found"))?;

        if !request.status.is_open() {
            return Err(Error::conflict(format!(
                "this swap is already {}",
                request.status.as_str()
            )));
        }

        let now = TimestampMicros::now();

        if request.end_at.0 <= now.0 {
            let expected = request.status;
            let expired = SwapRequest {
                status: SwapStatus::Expired,
                updated_at: now,
                ..request
            };

            self.swaps.update_if_status(expired, expected).await?;

            return Err(Error::conflict("this swap has expired"));
        }

        Ok(request)
    }

    /// 生效：申请记成已接受，再在排班最前面加替班，让它压过别的替班。排班写不进去时把申请放回原状。
    async fn apply(
        &self,
        schedule: &Schedule,
        request: SwapRequest,
        expected: SwapStatus,
        decided_by: &Id,
        now: TimestampMicros,
    ) -> Result<SwapOutcome> {
        let mut current = self.schedules.get(&schedule.id).await?;

        holds_window(&current, &request)?;

        let swap_override = ScheduleOverride {
            id: Id::new(),
            user_id: request.target_id.clone(),
            start_at: request.start_at,
            end_at: request.end_at,
            reason: if request.note.is_empty() {
                SWAP_REASON.to_string()
            } else {
                request.note.clone()
            },
        };

        let accepted = SwapRequest {
            status: SwapStatus::Accepted,
            override_id: Some(swap_override.id.clone()),
            decided_by: Some(decided_by.clone()),
            decided_at: Some(now),
            updated_at: now,
            ..request.clone()
        };

        if !self
            .swaps
            .update_if_status(accepted.clone(), expected)
            .await?
        {
            return Err(Error::conflict("this swap changed meanwhile"));
        }

        current.overrides.insert(0, swap_override);
        current.updated_at = now;

        let saved = match self.schedules.update(current).await {
            Ok(saved) => saved,
            Err(error) => {
                if let Err(restore) = self
                    .swaps
                    .update_if_status(request, SwapStatus::Accepted)
                    .await
                {
                    tracing::warn!(
                        swap_id = %accepted.id,
                        error = %restore,
                        "shift swap could not be put back after the schedule update failed"
                    );
                }

                return Err(error);
            }
        };

        self.announce(
            &saved,
            &accepted,
            vec![accepted.requester_id.clone(), accepted.target_id.clone()],
        )
        .await;

        Ok(SwapOutcome {
            request: accepted,
            schedule: Some(saved),
        })
    }

    /// 定下来但不生效：拒绝、驳回、撤回。
    async fn settle(
        &self,
        request: SwapRequest,
        expected: SwapStatus,
        status: SwapStatus,
        decided_by: &Id,
        now: TimestampMicros,
    ) -> Result<SwapRequest> {
        let settled = SwapRequest {
            status,
            decided_by: Some(decided_by.clone()),
            decided_at: Some(now),
            updated_at: now,
            ..request
        };

        if !self
            .swaps
            .update_if_status(settled.clone(), expected)
            .await?
        {
            return Err(Error::conflict("this swap changed meanwhile"));
        }

        Ok(settled)
    }

    async fn expire_ended(&self, requests: Vec<SwapRequest>) -> Vec<SwapRequest> {
        let now = TimestampMicros::now();
        let mut out = Vec::with_capacity(requests.len());

        for request in requests {
            if !request.status.is_open() || request.end_at.0 > now.0 {
                out.push(request);
                continue;
            }

            let expected = request.status;
            let expired = SwapRequest {
                status: SwapStatus::Expired,
                updated_at: now,
                ..request
            };

            if let Err(error) = self.swaps.update_if_status(expired.clone(), expected).await {
                tracing::warn!(swap_id = %expired.id, error = %error, "shift swap expiry failed");
            }

            out.push(expired);
        }

        out
    }

    async fn announce(&self, schedule: &Schedule, request: &SwapRequest, user_ids: Vec<Id>) {
        if let Err(error) = self
            .notify
            .enqueue_event(swap_dispatch(schedule, request, user_ids))
            .await
        {
            tracing::warn!(
                swap_id = %request.id,
                error = %error,
                "shift swap notify event enqueue failed"
            );
        }
    }
}

fn is_member(schedule: &Schedule, user_id: &Id) -> bool {
    schedule
        .rotations
        .iter()
        .any(|rotation| rotation.members.contains(user_id))
}

/// 这段时间里排班上值班的只有发起的人，才能把它换出去。
fn holds_window(schedule: &Schedule, request: &SwapRequest) -> Result<()> {
    let shifts = schedule.shifts(request.start_at, request.end_at);

    if shifts.is_empty()
        || shifts
            .iter()
            .any(|shift| shift.user_id != request.requester_id)
    {
        return Err(Error::invalid(
            "only a window you alone are on call for can be swapped",
        ));
    }

    Ok(())
}

/// 能批准的人：排班的创建人和最后改它的人。
fn managers(schedule: &Schedule) -> Vec<Id> {
    let mut ids = Vec::new();

    for id in [&schedule.created_by, &schedule.updated_by]
        .into_iter()
        .flatten()
    {
        push_unique(&mut ids, id);
    }

    ids
}

fn push_unique(ids: &mut Vec<Id>, id: &Id) {
    if !ids.contains(id) {
        ids.push(id.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::alerting::schedule::{Rotation, RotationKind};

    const HOUR: i64 = 3_600_000_000;

    fn schedule() -> Schedule {
        Schedule {
            id: Id::from_string("sch-1"),
            org_id: Id::from_string("org-a"),
            name: "Payments".into(),
            description: String::new(),
            team_id: None,
            timezone: "UTC".into(),
            enabled: true,
            rotations: vec![Rotation {
                id: Id::from_string("rot-1"),
                name: "Primary".into(),
                members: vec![Id::from_string("u1"), Id::from_string("u2")],
                kind: RotationKind::Daily,
                active_windows: Vec::new(),
                calendar: None,
                start_at: TimestampMicros(0),
                end_at: None,
            }],
            overrides: Vec::new(),
            swap_needs_approval: false,
            created_by: Some(Id::from_string("lead")),
            updated_by: Some(Id::from_string("lead")),
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

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
    fn only_a_window_the_requester_holds_alone_can_be_swapped() {
        let schedule = schedule();

        // 第一天 u1 值班，第二天 u2。
        assert!(holds_window(&schedule, &request(2 * HOUR, 20 * HOUR)).is_ok());
        assert!(holds_window(&schedule, &request(20 * HOUR, 30 * HOUR)).is_err());
        assert!(holds_window(&schedule, &request(25 * HOUR, 30 * HOUR)).is_err());
    }

    #[test]
    fn a_swap_override_wins_over_the_rotation_and_older_overrides() {
        let mut schedule = schedule();

        schedule.overrides.push(ScheduleOverride {
            id: Id::from_string("ov-old"),
            user_id: Id::from_string("u1"),
            start_at: TimestampMicros(0),
            end_at: TimestampMicros(48 * HOUR),
            reason: String::new(),
        });
        schedule.overrides.insert(
            0,
            ScheduleOverride {
                id: Id::from_string("ov-swap"),
                user_id: Id::from_string("u2"),
                start_at: TimestampMicros(2 * HOUR),
                end_at: TimestampMicros(6 * HOUR),
                reason: SWAP_REASON.into(),
            },
        );

        assert_eq!(
            schedule.who_is_on_call(TimestampMicros(3 * HOUR)),
            Some(Id::from_string("u2"))
        );
        assert_eq!(
            schedule.who_is_on_call(TimestampMicros(7 * HOUR)),
            Some(Id::from_string("u1"))
        );
    }

    #[test]
    fn managers_are_the_creator_and_last_editor_once_each() {
        let mut schedule = schedule();

        assert_eq!(managers(&schedule), vec![Id::from_string("lead")]);

        schedule.updated_by = Some(Id::from_string("ops"));
        assert_eq!(
            managers(&schedule),
            vec![Id::from_string("lead"), Id::from_string("ops")]
        );
        assert!(is_member(&schedule, &Id::from_string("u2")));
        assert!(!is_member(&schedule, &Id::from_string("lead")));
    }
}
