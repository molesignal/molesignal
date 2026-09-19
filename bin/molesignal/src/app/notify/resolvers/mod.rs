// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    domain::{
        alerting::repositories::ScheduleRepository,
        iam::IamMembershipRepository,
        notify::{
            policy::NotifyEvent,
            recipient::{NotifyRecipient, RecipientResolver},
        },
    },
    shared::{Error, Result, ids::Id},
};

mod oncall;
mod team;
mod users;

pub use oncall::{
    NEXT_ONCALL_RESOLVER, NextOncallResolver, SCHEDULE_MEMBERS_RESOLVER, ScheduleMembersResolver,
};
pub use team::{TEAM_LEAD_RESOLVER, TEAM_MEMBERS_RESOLVER, TeamLeadResolver, TeamMembersResolver};
pub use users::{
    ALERT_OWNER_RESOLVER, AlertOwnerResolver, EVENT_USERS_RESOLVER, EventUsersResolver,
};

pub const FIXED_USERS_RESOLVER: &str = "fixed_users";
pub const CURRENT_ONCALL_RESOLVER: &str = "current_oncall";

pub struct RecipientResolverRegistry {
    resolvers: HashMap<&'static str, Arc<dyn RecipientResolver>>,
}

impl RecipientResolverRegistry {
    pub fn new(resolvers: impl IntoIterator<Item = Arc<dyn RecipientResolver>>) -> Result<Self> {
        let mut registry = Self {
            resolvers: HashMap::new(),
        };
        for resolver in resolvers {
            let resolver_type = resolver.resolver_type();
            if resolver_type.trim().is_empty() {
                return Err(Error::internal("recipient resolver type cannot be empty"));
            }
            if registry.resolvers.insert(resolver_type, resolver).is_some() {
                return Err(Error::internal(format!(
                    "duplicate recipient resolver: {resolver_type}"
                )));
            }
        }
        Ok(registry)
    }

    pub fn get(&self, resolver_type: &str) -> Result<Arc<dyn RecipientResolver>> {
        self.resolvers.get(resolver_type).cloned().ok_or_else(|| {
            Error::invalid(format!(
                "unsupported notify recipient resolver: {resolver_type}"
            ))
        })
    }

    pub fn supported_types(&self) -> Vec<&'static str> {
        let mut types = self.resolvers.keys().copied().collect::<Vec<_>>();
        types.sort_unstable();
        types
    }
}

#[derive(Debug, Deserialize)]
struct FixedUsersConfig {
    user_ids: Vec<Id>,
    #[serde(default)]
    team_id: Option<Id>,
}

fn parse_fixed_users_config(config: &Value) -> Result<FixedUsersConfig> {
    let config: FixedUsersConfig = serde_json::from_value(config.clone())
        .map_err(|error| Error::invalid(format!("invalid fixed_users config: {error}")))?;
    if config.user_ids.is_empty() {
        return Err(Error::invalid(
            "fixed_users config requires at least one user_id",
        ));
    }
    if config.user_ids.len() > 500 {
        return Err(Error::invalid(
            "fixed_users config supports at most 500 users",
        ));
    }
    let unique = config
        .user_ids
        .iter()
        .map(|user_id| user_id.0.as_str())
        .collect::<HashSet<_>>();
    if unique.len() != config.user_ids.len() {
        return Err(Error::invalid("fixed_users user_ids must be unique"));
    }
    Ok(config)
}

pub struct FixedUsersResolver {
    memberships: Arc<dyn IamMembershipRepository>,
}

impl FixedUsersResolver {
    pub fn new(memberships: Arc<dyn IamMembershipRepository>) -> Self {
        Self { memberships }
    }
}

#[async_trait]
impl RecipientResolver for FixedUsersResolver {
    fn resolver_type(&self) -> &'static str {
        FIXED_USERS_RESOLVER
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        parse_fixed_users_config(config).map(|_| ())
    }

    async fn resolve(&self, event: &NotifyEvent, config: &Value) -> Result<Vec<NotifyRecipient>> {
        let config = parse_fixed_users_config(config)?;
        let members = self
            .memberships
            .list_for_org(&event.organization_id)
            .await?
            .into_iter()
            .map(|membership| membership.user_id)
            .collect::<HashSet<_>>();
        if config
            .user_ids
            .iter()
            .any(|user_id| !members.contains(user_id))
        {
            return Err(Error::invalid(
                "fixed_users contains a user outside the event organization",
            ));
        }
        Ok(config
            .user_ids
            .into_iter()
            .map(|user_id| NotifyRecipient {
                user_id,
                team_id: config.team_id.clone(),
            })
            .collect())
    }
}

#[derive(Debug, Deserialize)]
struct CurrentOncallConfig {
    schedule_id: Id,
}

fn parse_current_oncall_config(config: &Value) -> Result<CurrentOncallConfig> {
    serde_json::from_value(config.clone())
        .map_err(|error| Error::invalid(format!("invalid current_oncall config: {error}")))
}

pub struct CurrentOncallResolver {
    schedules: Arc<dyn ScheduleRepository>,
}

impl CurrentOncallResolver {
    pub fn new(schedules: Arc<dyn ScheduleRepository>) -> Self {
        Self { schedules }
    }
}

#[async_trait]
impl RecipientResolver for CurrentOncallResolver {
    fn resolver_type(&self) -> &'static str {
        CURRENT_ONCALL_RESOLVER
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        parse_current_oncall_config(config).map(|_| ())
    }

    async fn resolve(&self, event: &NotifyEvent, config: &Value) -> Result<Vec<NotifyRecipient>> {
        let config = parse_current_oncall_config(config)?;
        let schedule = self
            .schedules
            .list(&event.organization_id)
            .await?
            .into_iter()
            .find(|schedule| schedule.id == config.schedule_id)
            .ok_or_else(|| Error::not_found("on-call schedule"))?;
        Ok(schedule
            .who_is_on_call(event.occurred_at)
            .map(|user_id| {
                vec![NotifyRecipient {
                    user_id,
                    team_id: schedule.team_id,
                }]
            })
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::alerting::schedule::{Schedule, ScheduleOverride},
        shared::time::TimestampMicros,
    };

    struct ScheduleRepo {
        schedule: Schedule,
    }

    #[async_trait]
    impl ScheduleRepository for ScheduleRepo {
        async fn create(&self, schedule: Schedule) -> Result<Schedule> {
            Ok(schedule)
        }

        async fn update(&self, schedule: Schedule) -> Result<Schedule> {
            Ok(schedule)
        }

        async fn get(&self, id: &Id) -> Result<Schedule> {
            (self.schedule.id == *id)
                .then(|| self.schedule.clone())
                .ok_or_else(|| Error::not_found("schedule"))
        }

        async fn list(&self, org_id: &Id) -> Result<Vec<Schedule>> {
            Ok((self.schedule.org_id == *org_id)
                .then(|| self.schedule.clone())
                .into_iter()
                .collect())
        }

        async fn delete(&self, _id: &Id) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn fixed_users_config_rejects_duplicates() {
        assert!(
            parse_fixed_users_config(&serde_json::json!({
                "user_ids": ["user-a", "user-a"]
            }))
            .is_err()
        );
    }

    #[test]
    fn current_oncall_config_requires_schedule() {
        assert!(parse_current_oncall_config(&serde_json::json!({})).is_err());
    }

    #[tokio::test]
    async fn current_oncall_uses_effective_override_user() {
        let org_id = Id::from_string("org-a");
        let team_id = Id::from_string("team-a");
        let schedule = Schedule {
            id: Id::from_string("schedule-a"),
            org_id: org_id.clone(),
            name: "Primary".into(),
            description: String::new(),
            team_id: Some(team_id.clone()),
            timezone: "UTC".into(),
            enabled: true,
            rotations: Vec::new(),
            overrides: vec![ScheduleOverride {
                id: Id::from_string("override-a"),
                user_id: Id::from_string("substitute-user"),
                start_at: TimestampMicros(100),
                end_at: TimestampMicros(300),
                reason: "temporary cover".into(),
            }],
            created_by: None,
            updated_by: None,
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        };
        let resolver = CurrentOncallResolver::new(Arc::new(ScheduleRepo { schedule }));
        let recipients = resolver
            .resolve(
                &NotifyEvent {
                    id: "event-a".into(),
                    event_type: "alert.triggered".into(),
                    organization_id: org_id,
                    occurred_at: TimestampMicros(200),
                    attributes: serde_json::json!({}),
                },
                &serde_json::json!({"schedule_id": "schedule-a"}),
            )
            .await
            .unwrap();
        assert_eq!(recipients[0].user_id, Id::from_string("substitute-user"));
        assert_eq!(recipients[0].team_id, Some(team_id));
    }
}
