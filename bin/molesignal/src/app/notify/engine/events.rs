// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use super::{
    NotifyEngine,
    model::{NotifyDispatch, NotifyEventOutcome, NotifyPolicyOutcome, NotifyRecipientOutcome},
};
use crate::{
    app::notify::config::truncate_error,
    domain::notify::{
        delivery::{DeliveryFilter, DeliveryStage, DeliveryStatus, NotifyDelivery},
        event::{NotifyEventRecord, NotifyEventStatus},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const MAX_EVENT_ATTEMPTS: i32 = 5;
type RecipientDeliveries = BTreeMap<String, (Id, Vec<NotifyDelivery>)>;
type CompletedDeliveries = BTreeMap<String, (Id, RecipientDeliveries)>;

impl NotifyEngine {
    pub async fn retry_delivery(
        &self,
        organization_id: &Id,
        delivery_id: &Id,
    ) -> Result<NotifyEventOutcome> {
        let delivery = self.deliveries.get(organization_id, delivery_id).await?;
        if delivery.policy_id.is_none() {
            return Err(Error::invalid("notify test deliveries cannot be retried"));
        }
        if delivery.status != DeliveryStatus::Failed {
            return Err(Error::invalid(
                "only failed notify deliveries can be retried",
            ));
        }
        let claim = self
            .events
            .claim_retry(organization_id, &delivery.event_id, TimestampMicros::now())
            .await?;
        if !claim.acquired {
            return Err(Error::conflict("notify event is already being processed"));
        }
        self.process_claimed(claim.record).await
    }

    /// 持久化后立即处理；用于显式调用和测试。事件源通常只调用 `enqueue_event`，
    /// 再由 alert-manager 周期批量消费。
    pub async fn handle_event(&self, dispatch: NotifyDispatch) -> Result<NotifyEventOutcome> {
        self.enqueue_record(dispatch.clone()).await?;
        let now = TimestampMicros::now();
        let claim = self
            .events
            .claim(&dispatch.event.organization_id, &dispatch.event.id, now)
            .await?;
        if !claim.acquired {
            return match claim.record.status {
                NotifyEventStatus::Completed => {
                    self.completed_outcome(&dispatch.event.organization_id, &dispatch.event.id)
                        .await
                }
                NotifyEventStatus::Processing => {
                    Err(Error::conflict("notify event is already being processed"))
                }
                NotifyEventStatus::Pending | NotifyEventStatus::Failed => Err(Error::unavailable(
                    "notify event is waiting for a scheduled retry",
                )),
            };
        }
        self.process_claimed(claim.record).await
    }

    async fn completed_outcome(
        &self,
        organization_id: &Id,
        event_id: &str,
    ) -> Result<NotifyEventOutcome> {
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
        let mut grouped = CompletedDeliveries::new();
        for delivery in deliveries
            .into_iter()
            .filter(|delivery| delivery.event_id == event_id)
        {
            let (Some(policy_id), Some(user_id)) = (
                delivery.policy_id.clone(),
                delivery.recipient_user_id.clone(),
            ) else {
                continue;
            };
            grouped
                .entry(policy_id.0.clone())
                .or_insert_with(|| (policy_id, BTreeMap::new()))
                .1
                .entry(user_id.0.clone())
                .or_insert_with(|| (user_id, Vec::new()))
                .1
                .push(delivery);
        }
        let policies = grouped
            .into_values()
            .map(|(policy_id, recipients)| NotifyPolicyOutcome {
                policy_id,
                recipients: recipients
                    .into_values()
                    .map(|(user_id, mut attempts)| {
                        attempts.sort_by_key(|delivery| {
                            (
                                delivery_stage_rank(delivery.stage),
                                delivery.created_at.0,
                                delivery.id.0.clone(),
                            )
                        });
                        let delivered = attempts.iter().any(|delivery| {
                            matches!(
                                delivery.status,
                                DeliveryStatus::Success | DeliveryStatus::Acknowledged
                            )
                        });
                        NotifyRecipientOutcome {
                            user_id,
                            team_id: None,
                            delivered,
                            attempts,
                            error: (!delivered).then(|| "no notify route succeeded".into()),
                        }
                    })
                    .collect(),
                error: None,
            })
            .collect();
        Ok(NotifyEventOutcome {
            event_id: event_id.to_string(),
            policies,
        })
    }

    /// 事件源的无阻塞入口。成功入队返回 `true`。
    pub async fn enqueue_event(&self, dispatch: NotifyDispatch) -> Result<bool> {
        self.enqueue_record(dispatch).await?;
        Ok(true)
    }

    pub async fn process_pending_events(
        &self,
        organization_id: &Id,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<u32> {
        let records = self
            .events
            .claim_pending(organization_id, now, limit)
            .await?;
        let count = u32::try_from(records.len()).unwrap_or(u32::MAX);
        for record in records {
            if let Err(error) = self.process_claimed(record).await {
                tracing::warn!(
                    org_id = %organization_id,
                    error = %error,
                    "notify event processing failed"
                );
            }
        }
        Ok(count)
    }

    async fn enqueue_record(&self, dispatch: NotifyDispatch) -> Result<NotifyEventRecord> {
        super::execution::validate_event(&dispatch.event)?;
        let now = TimestampMicros::now();
        self.events
            .enqueue(NotifyEventRecord {
                event: dispatch.event,
                message: dispatch.message,
                status: NotifyEventStatus::Pending,
                attempt: 0,
                next_attempt_at: now,
                claimed_at: None,
                last_error: None,
                created_at: now,
                updated_at: now,
            })
            .await
    }

    async fn process_claimed(&self, record: NotifyEventRecord) -> Result<NotifyEventOutcome> {
        let organization_id = record.event.organization_id.clone();
        let event_id = record.event.id.clone();
        let attempt = record.attempt;
        let result = self
            .execute_dispatch_checked(NotifyDispatch {
                event: record.event,
                message: record.message,
            })
            .await;
        let now = TimestampMicros::now();
        match result {
            Ok(outcome) => {
                self.events
                    .finish(
                        &organization_id,
                        &event_id,
                        NotifyEventStatus::Completed,
                        now,
                        None,
                        now,
                    )
                    .await?;
                Ok(outcome)
            }
            Err(error) => {
                let terminal = attempt >= MAX_EVENT_ATTEMPTS;
                let next_attempt_at =
                    TimestampMicros(now.0.saturating_add(retry_delay_micros(attempt.max(1))));
                self.events
                    .finish(
                        &organization_id,
                        &event_id,
                        if terminal {
                            NotifyEventStatus::Failed
                        } else {
                            NotifyEventStatus::Pending
                        },
                        next_attempt_at,
                        Some(truncate_error(&error.to_string())),
                        now,
                    )
                    .await?;
                Err(error)
            }
        }
    }

    async fn execute_dispatch_checked(
        &self,
        dispatch: NotifyDispatch,
    ) -> Result<NotifyEventOutcome> {
        let outcome = self.execute_dispatch(dispatch).await?;
        if let Some(error) = delivery_failure(&outcome) {
            return Err(Error::unavailable(error));
        }
        Ok(outcome)
    }
}

const fn delivery_stage_rank(stage: DeliveryStage) -> u8 {
    match stage {
        DeliveryStage::UserPrimary => 0,
        DeliveryStage::UserFallback => 1,
        DeliveryStage::TeamFallback => 2,
        DeliveryStage::OrganizationFallback => 3,
        DeliveryStage::Escalation => 4,
        DeliveryStage::Test => 5,
    }
}

fn delivery_failure(outcome: &NotifyEventOutcome) -> Option<String> {
    for policy in &outcome.policies {
        if let Some(error) = &policy.error {
            return Some(format!(
                "notify policy {} failed: {error}",
                policy.policy_id
            ));
        }
        for recipient in &policy.recipients {
            if !recipient.delivered {
                return Some(format!(
                    "notify policy {} failed for recipient {}: {}",
                    policy.policy_id,
                    recipient.user_id,
                    recipient
                        .error
                        .as_deref()
                        .unwrap_or("no notify route succeeded")
                ));
            }
        }
    }
    None
}

fn retry_delay_micros(attempt: i32) -> i64 {
    let exponent = u32::try_from(attempt.saturating_sub(1).min(6)).unwrap_or(0);
    i64::from(5_u32.saturating_mul(2_u32.saturating_pow(exponent))) * 1_000_000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_delay_is_bounded_exponential() {
        assert_eq!(retry_delay_micros(1), 5_000_000);
        assert_eq!(retry_delay_micros(3), 20_000_000);
        assert_eq!(retry_delay_micros(100), 320_000_000);
    }
}
