// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 告警抑制的运行：规则在 [`crate::domain::alerting::inhibit`]，这里读规则和事件，把结果写回事件。
//!
//! - 开事件时只定「先等到什么时候」（[`Inhibitions::hold_new`]），不查别的事件：开事件的路径不该为抑制
//!   多读一遍全组织的事件。
//! - 谁在压谁由升级派发每一轮判定（[`Inhibitions::settle`]），只写本节点负责的事件，变了记时间线。
//! - 通知引擎和升级只看事件上的状态。

use std::{collections::HashMap, sync::Arc};

use serde_json::json;

use crate::{
    domain::alerting::{
        incident::Incident,
        inhibit::{self, InhibitPair, InhibitRule, InhibitRuleRepository, InhibitionChange},
        mute::{MuteRule, MuteRuleRepository, match_labels_for_incident},
        repositories::IncidentRepository,
        timeline::{IncidentTimelineRepository, TimelineEntry, TimelineKind},
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct Inhibitions {
    rules: Arc<dyn InhibitRuleRepository>,
    mutes: Arc<dyn MuteRuleRepository>,
    incidents: Arc<dyn IncidentRepository>,
    timeline: Arc<dyn IncidentTimelineRepository>,
}

impl Inhibitions {
    pub fn new(
        rules: Arc<dyn InhibitRuleRepository>,
        mutes: Arc<dyn MuteRuleRepository>,
        incidents: Arc<dyn IncidentRepository>,
        timeline: Arc<dyn IncidentTimelineRepository>,
    ) -> Self {
        Self {
            rules,
            mutes,
            incidents,
            timeline,
        }
    }

    pub fn rules(&self) -> &Arc<dyn InhibitRuleRepository> {
        &self.rules
    }

    /// 新开的事件：可能被压时，通知先等一会儿。规则读不出来时不等，宁可多发。
    pub async fn hold_new(&self, mut incident: Incident) -> Incident {
        match self.rules.list_enabled(&incident.org_id).await {
            Ok(rules) => {
                incident.handling.inhibition_wait_until = inhibit::wait_until(&rules, &incident)
            }
            Err(error) => {
                tracing::warn!(org_id = %incident.org_id, %error, "inhibit rules could not be read; not holding the new incident");
            }
        }

        incident
    }

    /// 一轮判定：`active` 是组织进行中的事件，`owned` 说哪些归本节点写。返回写过之后的事件。规则或静默
    /// 读不出来时这一轮不动，状态留到下一轮。
    pub async fn settle(
        &self,
        org_id: &Id,
        mut active: Vec<Incident>,
        owned: impl Fn(&Incident) -> bool,
        now: TimestampMicros,
    ) -> Vec<Incident> {
        let rules = match self.rules.list_enabled(org_id).await {
            Ok(rules) => rules,
            Err(error) => {
                tracing::warn!(%org_id, %error, "inhibit rules could not be read; keeping the state of this round");
                return active;
            }
        };

        // 没有规则、也没有在压或在等的事件：常见情况，不用再往下读。
        let untouched = rules.is_empty()
            && active.iter().all(|incident| {
                incident.handling.inhibition.is_none()
                    && incident.handling.inhibition_wait_until.is_none()
            });

        if untouched {
            return active;
        }

        let mutes = match self.mutes.list_enabled(org_id).await {
            Ok(mutes) => mutes,
            Err(error) => {
                tracing::warn!(%org_id, %error, "mute rules could not be read; keeping the inhibitions of this round");
                return active;
            }
        };

        let found = inhibit::resolve(&rules, &active, |incident| {
            is_silenced(&mutes, incident, now)
        });

        let live = inhibit::active_ids(&active);

        for incident in &mut active {
            if !incident.is_active() || incident.is_merged() || !owned(incident) {
                continue;
            }

            let mut next = incident.clone();

            let Some(change) = inhibit::settle(
                &mut next,
                found.get(&incident.id),
                &rules,
                |id| live.contains(id),
                now,
            ) else {
                continue;
            };

            let status = next.status;

            // 期间被别人改过：这一轮不写，下一轮按新的再算。
            match self.incidents.update_if_status(next, &[status]).await {
                Ok(Some(written)) => {
                    self.record(&written, &change, now).await;
                    *incident = written;
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(incident_id = %incident.id, %error, "the inhibition state could not be written");
                }
            }
        }

        active
    }

    /// 这条规则（可以还没存）现在会压住哪些事件。
    pub async fn preview(
        &self,
        rule: &InhibitRule,
        now: TimestampMicros,
    ) -> Result<Vec<InhibitPair>> {
        let incidents = self.incidents.list_active(&rule.org_id).await?;
        let mutes = self.mutes.list_enabled(&rule.org_id).await?;

        Ok(inhibit::preview(rule, &incidents, |incident| {
            is_silenced(&mutes, incident, now)
        }))
    }

    /// 每条规则此刻压着几个事件。
    pub async fn counts(&self, org_id: &Id) -> Result<HashMap<Id, usize>> {
        let incidents = self.incidents.list_active(org_id).await?;
        Ok(inhibit::inhibiting_counts(&incidents))
    }

    /// 开始和结束记进时间线；换原因、开始倒计时不记。
    async fn record(&self, incident: &Incident, change: &InhibitionChange, now: TimestampMicros) {
        let (kind, detail) = match change {
            InhibitionChange::Started => {
                let Some(state) = &incident.handling.inhibition else {
                    return;
                };

                (
                    TimelineKind::Inhibited,
                    json!({
                        "ruleId": state.rule_id,
                        "ruleName": state.rule_name,
                        "sourceIncidentId": state.source_incident_id,
                        "sourceSummary": state.source_summary,
                    }),
                )
            }
            InhibitionChange::Ended { reason } => {
                (TimelineKind::InhibitionEnded, json!({ "reason": reason }))
            }
            InhibitionChange::Updated | InhibitionChange::WaitOver => return,
        };

        let entry = TimelineEntry::new(&incident.org_id, &incident.id, now, kind, None, detail);

        if let Err(error) = self.timeline.append(entry).await {
            tracing::warn!(incident_id = %incident.id, %error, "incident timeline entry could not be written");
        }
    }
}

fn is_silenced(mutes: &[MuteRule], incident: &Incident, now: TimestampMicros) -> bool {
    let labels = match_labels_for_incident(&incident.labels, &incident.id);

    mutes.iter().any(|rule| rule.is_muting(&labels, now))
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Mutex};

    use async_trait::async_trait;

    use super::*;
    use crate::{
        domain::alerting::{
            incident::{IncidentStatus, Severity},
            inhibit::{InhibitSource, InhibitTarget},
            mute::MuteWindow,
            semantic_group::{LabelMatcher, MatchOp},
        },
        shared::Error,
    };

    #[derive(Default)]
    struct Rules {
        rules: Vec<InhibitRule>,
        broken: bool,
    }

    #[async_trait]
    impl InhibitRuleRepository for Rules {
        async fn create(&self, rule: InhibitRule) -> Result<InhibitRule> {
            Ok(rule)
        }

        async fn update(&self, rule: InhibitRule) -> Result<InhibitRule> {
            Ok(rule)
        }

        async fn get(&self, _: &Id) -> Result<InhibitRule> {
            Err(Error::not_found("no rule"))
        }

        async fn list(&self, _: &Id) -> Result<Vec<InhibitRule>> {
            Ok(self.rules.clone())
        }

        async fn delete(&self, _: &Id) -> Result<()> {
            Ok(())
        }

        async fn list_enabled(&self, _: &Id) -> Result<Vec<InhibitRule>> {
            if self.broken {
                return Err(Error::unavailable("db down"));
            }

            Ok(self.rules.clone())
        }
    }

    #[derive(Default)]
    struct Mutes(Vec<MuteRule>);

    #[async_trait]
    impl MuteRuleRepository for Mutes {
        async fn create(&self, rule: MuteRule) -> Result<MuteRule> {
            Ok(rule)
        }

        async fn update(&self, rule: MuteRule) -> Result<MuteRule> {
            Ok(rule)
        }

        async fn get(&self, _: &Id) -> Result<MuteRule> {
            Err(Error::not_found("no rule"))
        }

        async fn list(&self, _: &Id) -> Result<Vec<MuteRule>> {
            Ok(self.0.clone())
        }

        async fn delete(&self, _: &Id) -> Result<()> {
            Ok(())
        }

        async fn list_enabled(&self, _: &Id) -> Result<Vec<MuteRule>> {
            Ok(self.0.clone())
        }
    }

    /// 写回都成功：判定只看事件本身，不从这里读。
    struct Incidents;

    #[async_trait]
    impl IncidentRepository for Incidents {
        async fn create(&self, incident: Incident) -> Result<Incident> {
            Ok(incident)
        }

        async fn update(&self, incident: Incident) -> Result<Incident> {
            Ok(incident)
        }

        async fn update_if_status(
            &self,
            incident: Incident,
            _: &[IncidentStatus],
        ) -> Result<Option<Incident>> {
            Ok(Some(incident))
        }

        async fn get(&self, _: &Id) -> Result<Incident> {
            Err(Error::not_found("no incident"))
        }

        async fn list_active(&self, _: &Id) -> Result<Vec<Incident>> {
            Ok(Vec::new())
        }

        async fn list_merged_into(&self, _: &Id, _: &Id) -> Result<Vec<Incident>> {
            Ok(Vec::new())
        }

        async fn list_active_for_rule(&self, _: &Id, _: &Id) -> Result<Vec<Incident>> {
            Ok(Vec::new())
        }

        async fn find_by_fingerprint(&self, _: &Id, _: &str) -> Result<Option<Incident>> {
            Ok(None)
        }

        async fn list_by_status(&self, _: &Id, _: IncidentStatus) -> Result<Vec<Incident>> {
            Ok(Vec::new())
        }

        async fn list_since(&self, _: &Id, _: TimestampMicros) -> Result<Vec<Incident>> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct Timeline(Mutex<Vec<TimelineEntry>>);

    #[async_trait]
    impl IncidentTimelineRepository for Timeline {
        async fn append(&self, entry: TimelineEntry) -> Result<TimelineEntry> {
            self.0.lock().unwrap().push(entry.clone());
            Ok(entry)
        }

        async fn list(&self, _: &Id, _: &Id) -> Result<Vec<TimelineEntry>> {
            Ok(self.0.lock().unwrap().clone())
        }

        async fn recent_notes(&self, _: &Id, _: &Id, _: u32) -> Result<Vec<TimelineEntry>> {
            Ok(Vec::new())
        }
    }

    fn incident(id: &str, severity: Severity) -> Incident {
        Incident {
            id: Id::from_string(id),
            org_id: Id::from_string("org-1"),
            rule_id: Id::from_string(format!("rule-{id}")),
            escalation_policy_id: Id::from_string("default"),
            status: IncidentStatus::Open,
            severity,
            summary: format!("incident {id}"),
            fingerprint: format!("fp-{id}"),
            current_step: 0,
            current_loop: 0,
            current_step_started_at: TimestampMicros(1),
            assignees: Vec::new(),
            labels: BTreeMap::from([("service".to_string(), "checkout".to_string())]),
            annotations: BTreeMap::new(),
            trace_ids: Vec::new(),
            host_ids: Vec::new(),
            affected_services: Vec::new(),
            triggering_query: None,
            created_at: TimestampMicros(1),
            acknowledged_at: None,
            acknowledged_by: None,
            resolved_at: None,
            resolved_by: None,
            team_id: None,
            routing: None,
            handling: Default::default(),
            revision: 0,
        }
    }

    fn rule() -> InhibitRule {
        InhibitRule {
            id: Id::from_string("inhibit-1"),
            org_id: Id::from_string("org-1"),
            name: "same service".into(),
            enabled: true,
            source: InhibitSource::default(),
            target: InhibitTarget::default(),
            equal: vec!["service".into()],
            more_severe_only: true,
            wait_secs: 60,
            linger_secs: 0,
            comment: String::new(),
            created_by: None,
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        }
    }

    fn service(rules: Rules, mutes: Vec<MuteRule>) -> (Inhibitions, Arc<Timeline>) {
        let timeline = Arc::new(Timeline::default());

        let service = Inhibitions::new(
            Arc::new(rules),
            Arc::new(Mutes(mutes)),
            Arc::new(Incidents),
            timeline.clone(),
        );

        (service, timeline)
    }

    #[tokio::test]
    async fn a_new_incident_waits_only_under_a_rule_that_could_hold_it() {
        let (held, _) = service(
            Rules {
                rules: vec![rule()],
                broken: false,
            },
            Vec::new(),
        );
        let opened = held.hold_new(incident("slow", Severity::Warning)).await;
        assert!(opened.handling.inhibition_wait_until.is_some());

        let worst = held.hold_new(incident("db", Severity::Critical)).await;
        assert_eq!(worst.handling.inhibition_wait_until, None);

        let (broken, _) = service(
            Rules {
                rules: vec![rule()],
                broken: true,
            },
            Vec::new(),
        );
        let opened = broken.hold_new(incident("slow", Severity::Warning)).await;
        assert_eq!(opened.handling.inhibition_wait_until, None);
    }

    #[tokio::test]
    async fn a_round_marks_the_lesser_incident_and_lets_it_go_when_the_source_recovers() {
        let (service, timeline) = service(
            Rules {
                rules: vec![rule()],
                broken: false,
            },
            Vec::new(),
        );
        let now = TimestampMicros(10_000_000);
        let source = incident("db", Severity::Critical);
        let target = incident("slow", Severity::Warning);

        let settled = service
            .settle(&source.org_id, vec![source.clone(), target], |_| true, now)
            .await;

        let held = settled.iter().find(|i| i.id.as_str() == "slow").unwrap();
        let state = held.handling.inhibition.as_ref().unwrap();
        assert_eq!(state.source_incident_id.as_str(), "db");
        assert_eq!(timeline.0.lock().unwrap()[0].kind, TimelineKind::Inhibited);

        // 再来一轮不用写，也不再记。
        let again = service.settle(&source.org_id, settled, |_| true, now).await;
        assert_eq!(timeline.0.lock().unwrap().len(), 1);

        // 源事件恢复：这条规则不延后，这一轮放开。
        let target = again.into_iter().find(|i| i.id.as_str() == "slow").unwrap();
        let later = TimestampMicros(20_000_000);
        let released = service
            .settle(&source.org_id, vec![target], |_| true, later)
            .await;

        assert_eq!(released[0].handling.inhibition, None);
        assert_eq!(released[0].current_step_started_at, later);

        let entries = timeline.0.lock().unwrap();
        assert_eq!(entries[1].kind, TimelineKind::InhibitionEnded);
        assert_eq!(entries[1].detail["reason"], "source_recovered");
    }

    #[tokio::test]
    async fn an_incident_of_another_node_is_left_to_it() {
        let (service, timeline) = service(
            Rules {
                rules: vec![rule()],
                broken: false,
            },
            Vec::new(),
        );
        let source = incident("db", Severity::Critical);
        let target = incident("slow", Severity::Warning);

        let settled = service
            .settle(
                &source.org_id.clone(),
                vec![source, target],
                |_| false,
                TimestampMicros(10),
            )
            .await;

        assert!(settled.iter().all(|i| i.handling.inhibition.is_none()));
        assert!(timeline.0.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_silenced_source_holds_nothing_back() {
        let silence = MuteRule {
            id: Id::from_string("m1"),
            org_id: Id::from_string("org-1"),
            name: "db".into(),
            enabled: true,
            matchers: vec![LabelMatcher {
                label: crate::domain::alerting::mute::INCIDENT_ID_MATCHER_LABEL.into(),
                op: MatchOp::Eq,
                value: "db".into(),
            }],
            window: MuteWindow::Fixed {
                start: TimestampMicros(0),
                end: TimestampMicros(i64::MAX),
            },
            comment: String::new(),
            created_by: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        };

        let (service, _) = service(
            Rules {
                rules: vec![rule()],
                broken: false,
            },
            vec![silence],
        );

        let source = incident("db", Severity::Critical);
        let target = incident("slow", Severity::Warning);

        let settled = service
            .settle(
                &source.org_id.clone(),
                vec![source, target],
                |_| true,
                TimestampMicros(10),
            )
            .await;

        assert!(settled.iter().all(|i| i.handling.inhibition.is_none()));
    }

    #[tokio::test]
    async fn rules_that_cannot_be_read_keep_the_state() {
        let (service, timeline) = service(
            Rules {
                rules: Vec::new(),
                broken: true,
            },
            Vec::new(),
        );

        let mut held = incident("slow", Severity::Warning);
        held.handling.inhibition_wait_until = Some(TimestampMicros(1));

        let settled = service
            .settle(
                &held.org_id.clone(),
                vec![held],
                |_| true,
                TimestampMicros(10),
            )
            .await;

        assert!(settled[0].handling.inhibition_wait_until.is_some());
        assert!(timeline.0.lock().unwrap().is_empty());
    }
}
