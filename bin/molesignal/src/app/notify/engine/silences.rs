// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 静默：命中启用静默的告警事件，通知先不发。触发与升级的通知压着，静默结束时事件还开着就照常发，
//! 已确认或已恢复就不再发；确认与恢复的通知只在触发通知已经发出时才发。个人接收顺序里的后续步骤和
//! 策略的确认超时升级也等静默结束。
//!
//! 发失败、等着重试的触发与升级通知也一样：告警事件已确认或已恢复就不再发，不去叫一个已经没事的人。
//!
//! 被抑制的告警事件（[`super::inhibitions`]）的通知和静默的一样处理。并入了别的事件的告警事件不再有任何
//! 通知：由主事件通知。

use std::{collections::BTreeMap, sync::Arc};

use serde_json::Value;

use super::NotifyEngine;
use crate::{
    app::notify::{
        ALERT_ACKNOWLEDGED_EVENT, ALERT_ESCALATED_EVENT, ALERT_RESOLVED_EVENT,
        ALERT_TRIGGERED_EVENT, triggered_event_id,
    },
    domain::{
        alerting::{
            incident::IncidentStatus,
            mute::{MuteRule, MuteRuleRepository, match_labels_for_incident},
            repositories::IncidentRepository,
        },
        notify::{
            delivery::{DeliveryFilter, DeliveryStatus},
            event::{NotifyEventRecord, NotifyEventStatus},
            policy::NotifyEvent,
        },
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 压着的通知至少隔这么久再看：比升级派发的默认间隔（10 秒）短，静默结束或被删掉后随下一次派发发出。
pub(super) const RECHECK_MICROS: i64 = 5_000_000;

/// 压着的通知事件的 `last_error`：它在等静默或抑制结束。
const HELD_NOTE: &str = "held by a silence or an inhibition";

pub(super) struct Silences {
    mute_rules: Arc<dyn MuteRuleRepository>,
    pub(super) incidents: Arc<dyn IncidentRepository>,
}

impl Silences {
    pub(super) fn new(
        mute_rules: Arc<dyn MuteRuleRepository>,
        incidents: Arc<dyn IncidentRepository>,
    ) -> Self {
        Self {
            mute_rules,
            incidents,
        }
    }
}

/// 一条告警通知此刻怎么办。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Verdict {
    /// 照常处理。
    Send,
    /// 压着，等静默结束再看。
    Hold,
    /// 不再发。
    Drop,
}

/// 一个组织此刻启用的静默：一批通知里第一条告警通知用到时才读，之后共用。
pub(super) struct OrgSilences<'a> {
    silences: Option<&'a Silences>,
    organization_id: &'a Id,
    rules: Option<Vec<MuteRule>>,
}

impl OrgSilences<'_> {
    /// 这条告警通知此刻被静默。不是告警的通知不算；静默读不出来时也不算，和升级派发一样。
    pub(super) async fn silenced(&mut self, event: &NotifyEvent, now: TimestampMicros) -> bool {
        let (Some(silences), Some(alert_id)) = (self.silences, alert_id_of(event)) else {
            return false;
        };

        if self.rules.is_none() {
            let rules = match silences.mute_rules.list_enabled(self.organization_id).await {
                Ok(rules) => rules,
                Err(error) => {
                    tracing::warn!(
                        org_id = %self.organization_id,
                        error = %error,
                        "mute rule check failed; not silencing notifications"
                    );
                    Vec::new()
                }
            };

            self.rules = Some(rules);
        }

        let labels = match_labels_for_incident(&event_labels(event), &alert_id);

        self.rules
            .iter()
            .flatten()
            .any(|rule| rule.is_muting(&labels, now))
            || silences.inhibited(&alert_id, now).await
    }
}

impl NotifyEngine {
    /// 这个组织的静默，给一批通知共用。
    pub(super) fn org_silences<'a>(&'a self, organization_id: &'a Id) -> OrgSilences<'a> {
        OrgSilences {
            silences: self.silences.as_ref(),
            organization_id,
            rules: None,
        }
    }

    /// 领到的一条通知事件此刻怎么办。看不出来时先压着，过一会儿再看。
    pub(super) async fn silence_verdict(
        &self,
        silences: &mut OrgSilences<'_>,
        record: &NotifyEventRecord,
        now: TimestampMicros,
    ) -> Verdict {
        match self.verdict(silences, record, now).await {
            Ok(verdict) => verdict,
            Err(error) => {
                tracing::warn!(
                    org_id = %record.event.organization_id,
                    event_id = %record.event.id,
                    error = %error,
                    "notify event silence check failed; holding it"
                );

                Verdict::Hold
            }
        }
    }

    /// 压着的放回去过一会儿再看，不再发的记为处理完。
    pub(super) async fn settle_silenced(
        &self,
        event: &NotifyEvent,
        verdict: Verdict,
        now: TimestampMicros,
    ) -> Result<()> {
        match verdict {
            Verdict::Send => {}
            Verdict::Hold => {
                self.events
                    .postpone(
                        &event.organization_id,
                        &event.id,
                        TimestampMicros(now.0.saturating_add(RECHECK_MICROS)),
                        HELD_NOTE,
                        now,
                    )
                    .await?;
            }
            Verdict::Drop => {
                self.events
                    .finish(
                        &event.organization_id,
                        &event.id,
                        NotifyEventStatus::Completed,
                        now,
                        None,
                        now,
                    )
                    .await?;
            }
        }

        Ok(())
    }

    async fn verdict(
        &self,
        silences: &mut OrgSilences<'_>,
        record: &NotifyEventRecord,
        now: TimestampMicros,
    ) -> Result<Verdict> {
        let event = &record.event;

        let Some(alert_id) = alert_id_of(event) else {
            return Ok(Verdict::Send);
        };

        if self.alert_merged(&alert_id).await? {
            return Ok(Verdict::Drop);
        }

        match event.event_type.as_str() {
            ALERT_TRIGGERED_EVENT | ALERT_ESCALATED_EVENT => {
                // 压过的或发失败等重试的通知：告警事件在此期间确认了或恢复了，就不再发。
                if record.last_error.is_some() && !self.alert_open(&alert_id).await? {
                    return Ok(Verdict::Drop);
                }

                if silences.silenced(event, now).await {
                    Ok(Verdict::Hold)
                } else {
                    Ok(Verdict::Send)
                }
            }
            ALERT_ACKNOWLEDGED_EVENT | ALERT_RESOLVED_EVENT => {
                // 触发通知被静默挡下的，确认和恢复也不发。
                if silences.silenced(event, now).await
                    && !self
                        .triggered_sent(&event.organization_id, &alert_id)
                        .await?
                {
                    Ok(Verdict::Drop)
                } else {
                    Ok(Verdict::Send)
                }
            }
            _ => Ok(Verdict::Send),
        }
    }

    /// 告警事件还开着：没确认，也没恢复。事件不在了算没开着。
    async fn alert_open(&self, alert_id: &Id) -> Result<bool> {
        let Some(silences) = &self.silences else {
            return Ok(true);
        };

        match silences.incidents.get(alert_id).await {
            Ok(incident) => Ok(incident.status == IncidentStatus::Open),
            Err(Error::NotFound(_)) => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// 告警事件并入了别的事件。
    async fn alert_merged(&self, alert_id: &Id) -> Result<bool> {
        let Some(silences) = &self.silences else {
            return Ok(false);
        };

        match silences.incidents.get(alert_id).await {
            Ok(incident) => Ok(incident.is_merged()),
            Err(Error::NotFound(_)) => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// 告警事件的触发通知已经投递过。
    async fn triggered_sent(&self, organization_id: &Id, alert_id: &Id) -> Result<bool> {
        let event_id = triggered_event_id(alert_id);

        let deliveries = self
            .deliveries
            .list(
                organization_id,
                &DeliveryFilter {
                    event_id: Some(event_id.clone()),
                    limit: 100,
                    ..DeliveryFilter::default()
                },
            )
            .await?;

        // 流水线不让发的策略留下的是跳过记录，不算发过。
        Ok(deliveries.iter().any(|delivery| {
            delivery.event_id == event_id && delivery.status != DeliveryStatus::Skipped
        }))
    }
}

/// 通知事件所属的告警事件；不是告警的通知没有。
fn alert_id_of(event: &NotifyEvent) -> Option<Id> {
    if !event.event_type.starts_with("alert.") {
        return None;
    }

    event
        .attributes
        .get("alert_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(Id::from_string)
}

/// 通知事件里记下的告警标签。
fn event_labels(event: &NotifyEvent) -> BTreeMap<String, String> {
    let Some(labels) = event.attributes.get("labels").and_then(Value::as_object) else {
        return BTreeMap::new();
    };

    labels
        .iter()
        .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_string())))
        .collect()
}
