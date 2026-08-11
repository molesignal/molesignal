// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{collections::HashSet, sync::Arc};

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    domain::{
        iam::IamMembershipRepository,
        notify::{
            policy::NotifyEvent,
            recipient::{NotifyRecipient, RecipientResolver},
        },
    },
    shared::{Error, Result, ids::Id},
};

pub const EVENT_USERS_RESOLVER: &str = "event_users";
pub const ALERT_OWNER_RESOLVER: &str = "alert_owner";

#[derive(Debug, Deserialize)]
struct EventUsersConfig {
    #[serde(default = "default_user_attribute")]
    attribute: String,
    #[serde(default)]
    user_ids: Vec<Id>,
    #[serde(default)]
    team_id: Option<Id>,
}

#[derive(Debug, Deserialize)]
struct AlertOwnerConfig {
    #[serde(default = "default_owner_attribute")]
    attribute: String,
    #[serde(default)]
    team_id: Option<Id>,
}

fn default_user_attribute() -> String {
    "user_ids".into()
}

fn default_owner_attribute() -> String {
    "owner_user_id".into()
}

fn validate_attribute(attribute: &str) -> Result<()> {
    if attribute.trim().is_empty() || attribute.len() > 128 {
        return Err(Error::invalid(
            "notify event user attribute must contain between 1 and 128 bytes",
        ));
    }
    Ok(())
}

fn values_at(event: &NotifyEvent, attribute: &str) -> Vec<Id> {
    let Some(value) = event.attributes.get(attribute) else {
        return Vec::new();
    };
    match value {
        Value::String(value) if !value.trim().is_empty() => vec![Id::from_string(value)],
        Value::Array(values) => values
            .iter()
            .filter_map(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(Id::from_string)
            .collect(),
        _ => Vec::new(),
    }
}

async fn validate_members(
    memberships: &dyn IamMembershipRepository,
    event: &NotifyEvent,
    user_ids: Vec<Id>,
    team_id: Option<Id>,
) -> Result<Vec<NotifyRecipient>> {
    if user_ids.len() > 500 {
        return Err(Error::invalid(
            "notify event user resolver supports at most 500 users",
        ));
    }
    let organization_users = memberships
        .list_for_org(&event.organization_id)
        .await?
        .into_iter()
        .map(|membership| membership.user_id)
        .collect::<HashSet<_>>();
    let mut seen = HashSet::new();
    let mut recipients = Vec::new();
    for user_id in user_ids {
        if !seen.insert(user_id.clone()) {
            continue;
        }
        if !organization_users.contains(&user_id) {
            return Err(Error::invalid(
                "notify event references a user outside the event organization",
            ));
        }
        recipients.push(NotifyRecipient {
            user_id,
            team_id: team_id.clone(),
        });
    }
    Ok(recipients)
}

pub struct EventUsersResolver {
    memberships: Arc<dyn IamMembershipRepository>,
}

impl EventUsersResolver {
    pub fn new(memberships: Arc<dyn IamMembershipRepository>) -> Self {
        Self { memberships }
    }
}

#[async_trait]
impl RecipientResolver for EventUsersResolver {
    fn resolver_type(&self) -> &'static str {
        EVENT_USERS_RESOLVER
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        let config: EventUsersConfig = serde_json::from_value(config.clone())
            .map_err(|error| Error::invalid(format!("invalid event_users config: {error}")))?;
        validate_attribute(&config.attribute)
    }

    async fn resolve(&self, event: &NotifyEvent, config: &Value) -> Result<Vec<NotifyRecipient>> {
        let config: EventUsersConfig = serde_json::from_value(config.clone())
            .map_err(|error| Error::invalid(format!("invalid event_users config: {error}")))?;
        validate_attribute(&config.attribute)?;
        let mut user_ids = config.user_ids;
        user_ids.extend(values_at(event, &config.attribute));
        validate_members(self.memberships.as_ref(), event, user_ids, config.team_id).await
    }
}

pub struct AlertOwnerResolver {
    memberships: Arc<dyn IamMembershipRepository>,
}

impl AlertOwnerResolver {
    pub fn new(memberships: Arc<dyn IamMembershipRepository>) -> Self {
        Self { memberships }
    }
}

#[async_trait]
impl RecipientResolver for AlertOwnerResolver {
    fn resolver_type(&self) -> &'static str {
        ALERT_OWNER_RESOLVER
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        let config: AlertOwnerConfig = serde_json::from_value(config.clone())
            .map_err(|error| Error::invalid(format!("invalid alert_owner config: {error}")))?;
        validate_attribute(&config.attribute)
    }

    async fn resolve(&self, event: &NotifyEvent, config: &Value) -> Result<Vec<NotifyRecipient>> {
        let config: AlertOwnerConfig = serde_json::from_value(config.clone())
            .map_err(|error| Error::invalid(format!("invalid alert_owner config: {error}")))?;
        validate_attribute(&config.attribute)?;
        let mut user_ids = values_at(event, &config.attribute);
        if user_ids.is_empty() && config.attribute == "owner_user_id" {
            user_ids.extend(values_at(event, "assignee_user_ids"));
        }
        validate_members(self.memberships.as_ref(), event, user_ids, config.team_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_values_accept_string_and_array() {
        let base = NotifyEvent {
            id: "event".into(),
            event_type: "alert.triggered".into(),
            organization_id: Id::from_string("org"),
            occurred_at: crate::shared::time::TimestampMicros(1),
            attributes: serde_json::json!({"owners": ["a", "b"]}),
        };
        assert_eq!(values_at(&base, "owners").len(), 2);
        let single = NotifyEvent {
            attributes: serde_json::json!({"owners": "a"}),
            ..base
        };
        assert_eq!(values_at(&single, "owners"), vec![Id::from_string("a")]);
    }
}
