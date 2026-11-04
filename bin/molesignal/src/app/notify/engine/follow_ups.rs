// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 个人的后续步骤：告警送到一个人之后，接收顺序里等得更久的下一步按它的等待时间排好；到时仍没人确认，
//! 就用它再发，再排下一步。发送失败的一步照旧立即换下一个。等待时间与上一步相同的步骤只做失败兜底。
//! 到期时告警在静默里的，等静默结束再发。

use super::{
    NotifyEngine,
    model::ResolvedRoute,
    pipelines::PolicyCopy,
    silences::{OrgSilences, RECHECK_MICROS},
};
use crate::{
    app::notify::{ALERT_ESCALATED_EVENT, ALERT_TRIGGERED_EVENT},
    domain::notify::{
        delivery::{DeliveryFilter, DeliveryStage},
        follow_up::NotifyFollowUp,
        policy::{NotifyDeliveryMode, NotifyEvent, NotifyPolicy},
        recipient::NotifyRecipient,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// 一个到期的后续步骤处理完以后。
enum StepRun {
    /// 发了，或者不用再发。
    Done,
    /// 告警在静默里，没有发。
    Held,
}

/// 等人确认的事件：告警触发与升级。恢复、值班提醒之类不会有人确认，不排后续步骤。
pub(super) fn awaits_acknowledgement(event: &NotifyEvent) -> bool {
    matches!(
        event.event_type.as_str(),
        ALERT_TRIGGERED_EVENT | ALERT_ESCALATED_EVENT
    )
}

/// 个人接收顺序里的一步（不是团队或组织兜底）。
pub(super) fn is_user_step(route: &ResolvedRoute) -> bool {
    matches!(
        route.stage,
        DeliveryStage::UserPrimary | DeliveryStage::UserFallback
    )
}

/// 这次投递会排后续步骤时，接收顺序里各步不同的等待时间，从小到大；不排时为空。按接收人自己的顺序
/// 投递、等人确认、且不是策略的确认超时升级时才排。
pub(super) fn step_delays(
    policy: &NotifyPolicy,
    event: &NotifyEvent,
    escalation: bool,
    routes: &[ResolvedRoute],
) -> Vec<i32> {
    if escalation
        || policy.delivery_mode != NotifyDeliveryMode::PreferUser
        || !awaits_acknowledgement(event)
    {
        return Vec::new();
    }

    let mut delays = routes
        .iter()
        .filter(|route| is_user_step(route))
        .map(|route| route.delay_secs)
        .collect::<Vec<_>>();

    delays.sort_unstable();
    delays.dedup();
    delays
}

impl NotifyEngine {
    /// 送到了等待时间为 `delivered_delay` 的那一步：排等得更久的下一步，从 `started_at` 算起。
    pub(super) async fn schedule_follow_up(
        &self,
        policy: &NotifyPolicy,
        event: &NotifyEvent,
        recipient: &NotifyRecipient,
        delays: &[i32],
        delivered_delay: i32,
        started_at: TimestampMicros,
    ) {
        let Some(next) = delays
            .iter()
            .copied()
            .find(|delay| *delay > delivered_delay)
        else {
            return;
        };

        let follow_up = NotifyFollowUp {
            organization_id: event.organization_id.clone(),
            event_id: event.id.clone(),
            policy_id: policy.id.clone(),
            recipient_user_id: recipient.user_id.clone(),
            delay_secs: next,
            due_at: TimestampMicros(started_at.0.saturating_add(i64::from(next) * 1_000_000)),
        };

        if let Err(error) = self.follow_ups.schedule(follow_up).await {
            tracing::warn!(
                org_id = %event.organization_id,
                event_id = %event.id,
                error = %error,
                "notify follow-up step could not be scheduled"
            );
        }
    }

    /// 处理到期的后续步骤，返回处理的个数。
    pub async fn process_due_follow_ups(
        &self,
        organization_id: &Id,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<u32> {
        let due = self
            .follow_ups
            .claim_due(organization_id, now, limit)
            .await?;

        let count = u32::try_from(due.len()).unwrap_or(u32::MAX);
        let mut silences = self.org_silences(organization_id);

        for follow_up in due {
            match self.run_follow_up(&follow_up, &mut silences, now).await {
                Ok(StepRun::Done) => {}
                Ok(StepRun::Held) => {
                    self.postpone_follow_up(&follow_up, now).await;
                    continue;
                }
                Err(error) => {
                    tracing::warn!(
                        org_id = %organization_id,
                        event_id = %follow_up.event_id,
                        error = %error,
                        "notify follow-up step failed"
                    );
                }
            }

            if let Err(error) = self.follow_ups.finish(&follow_up).await {
                tracing::warn!(
                    org_id = %organization_id,
                    event_id = %follow_up.event_id,
                    error = %error,
                    "notify follow-up step could not be marked done"
                );
            }
        }

        Ok(count)
    }

    /// 静默压着的步骤过一会儿再看；它往后挪，后面的步骤也从挪后的时刻算起。
    async fn postpone_follow_up(&self, follow_up: &NotifyFollowUp, now: TimestampMicros) {
        let due_at = TimestampMicros(now.0.saturating_add(RECHECK_MICROS));

        if let Err(error) = self.follow_ups.postpone(follow_up, due_at).await {
            tracing::warn!(
                org_id = %follow_up.organization_id,
                event_id = %follow_up.event_id,
                error = %error,
                "notify follow-up step could not be postponed"
            );
        }
    }

    async fn run_follow_up(
        &self,
        follow_up: &NotifyFollowUp,
        silences: &mut OrgSilences<'_>,
        now: TimestampMicros,
    ) -> Result<StepRun> {
        let organization_id = &follow_up.organization_id;

        if self
            .event_acknowledged(organization_id, &follow_up.event_id)
            .await?
        {
            return Ok(StepRun::Done);
        }

        let record = self
            .events
            .get(organization_id, &follow_up.event_id)
            .await?;

        if silences.silenced(&record.event, now).await {
            return Ok(StepRun::Held);
        }

        let policy = self
            .policies
            .get(organization_id, &follow_up.policy_id)
            .await?;

        if !policy.enabled {
            return Ok(StepRun::Done);
        }

        // 后续步骤发出去的和第一步一样，是流水线留下的那一份。
        let copy = match self
            .policy_copy(&policy, &record.event, &record.message)
            .await
        {
            PolicyCopy::AsIs => None,
            PolicyCopy::Changed(copy) => Some(copy),
            PolicyCopy::Dropped(_) => return Ok(StepRun::Done),
        };

        let (event, message) = copy
            .as_deref()
            .map_or((&record.event, &record.message), |(event, message)| {
                (event, message)
            });

        let messages = self.policy_messages(&policy, event, message).await?;

        let recipient = NotifyRecipient {
            user_id: follow_up.recipient_user_id.clone(),
            team_id: None,
        };

        let routes = self.user_routes(&policy, &record.event, &recipient).await?;

        let delays = step_delays(&policy, &record.event, false, &routes);

        // 这一步以及更晚的步骤，按顺序：发送失败照旧立即换下一个。
        for route in routes
            .into_iter()
            .filter(|route| route.delay_secs >= follow_up.delay_secs)
        {
            let delay_secs = route.delay_secs;

            let (delivered, _) = self
                .attempt_route(&policy, event, &recipient, route, &messages)
                .await?;

            if delivered {
                self.schedule_follow_up(
                    &policy,
                    &record.event,
                    &recipient,
                    &delays,
                    delay_secs,
                    follow_up.started_at(),
                )
                .await;

                return Ok(StepRun::Done);
            }
        }

        Ok(StepRun::Done)
    }

    /// 事件的投递里有被确认的：告警已经有人接手。
    async fn event_acknowledged(&self, organization_id: &Id, event_id: &str) -> Result<bool> {
        let deliveries = self
            .deliveries
            .list(
                organization_id,
                &DeliveryFilter {
                    event_id: Some(event_id.to_string()),
                    limit: 500,
                    ..DeliveryFilter::default()
                },
            )
            .await?;

        Ok(deliveries
            .iter()
            .any(|delivery| delivery.event_id == event_id && delivery.acknowledged_at.is_some()))
    }
}
