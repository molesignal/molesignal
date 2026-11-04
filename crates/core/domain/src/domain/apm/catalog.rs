// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What people keep about the services APM sees. Projections are rebuilt
//! from spans; these are written by people and stay: who owns a service, how
//! much it matters, where it is in its life, and what is being done about
//! each of its error groups.

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{OTHER_DIMENSION, TransactionIdentity, masked_repository_address};
use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

/// The longest description a service keeps.
pub const SERVICE_DESCRIPTION_MAX_LENGTH: usize = 2_000;

/// The most tags a service keeps, and the longest a tag's key and value are.
pub const SERVICE_TAGS_MAX: usize = 32;
pub const SERVICE_TAG_KEY_MAX_LENGTH: usize = 64;
pub const SERVICE_TAG_VALUE_MAX_LENGTH: usize = 128;

/// The most endpoints a service keeps as key transactions.
pub const SERVICE_KEY_TRANSACTIONS_MAX: usize = 50;

/// The longest Apdex threshold a service sets: a minute.
pub const SERVICE_APDEX_THRESHOLD_MAX_MICROS: u64 = 60_000_000;

/// How much a service matters, from P0, the most, to P5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceTier {
    P0,
    P1,
    P2,
    P3,
    P4,
    P5,
}

impl ServiceTier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::P0 => "p0",
            Self::P1 => "p1",
            Self::P2 => "p2",
            Self::P3 => "p3",
            Self::P4 => "p4",
            Self::P5 => "p5",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "p0" => Some(Self::P0),
            "p1" => Some(Self::P1),
            "p2" => Some(Self::P2),
            "p3" => Some(Self::P3),
            "p4" => Some(Self::P4),
            "p5" => Some(Self::P5),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceLifecycle {
    Production,
    Staging,
    Experimental,
    Deprecated,
}

impl ServiceLifecycle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Production => "production",
            Self::Staging => "staging",
            Self::Experimental => "experimental",
            Self::Deprecated => "deprecated",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "production" => Some(Self::Production),
            "staging" => Some(Self::Staging),
            "experimental" => Some(Self::Experimental),
            "deprecated" => Some(Self::Deprecated),
            _ => None,
        }
    }
}

/// A service as people describe it. Keyed by namespace and name, so it holds
/// in every environment the service runs in, and before it first reports.
/// Who owns it are the platform's own teams and users.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceDefinition {
    pub namespace: String,
    pub name: String,
    pub description: String,
    pub team_id: Option<Id>,
    pub owner_id: Option<Id>,
    pub tier: Option<ServiceTier>,
    pub lifecycle: Option<ServiceLifecycle>,
    /// The code's repository, for a service whose telemetry does not say it:
    /// an http(s) or git address, its credentials masked.
    #[serde(default)]
    pub repository_url: Option<String>,
    /// What people label the service with, `key=value`, to filter and group
    /// the catalog by. A value may be empty: the key alone is the label.
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
    /// The Apdex threshold T of the service's requests, in place of the default.
    #[serde(default)]
    pub apdex_threshold: Option<u64>,
    /// The endpoints that matter most, starred in the service's endpoint lists.
    #[serde(default)]
    pub key_transactions: Vec<TransactionIdentity>,
    /// Out of the catalog's default view: retired, or no longer watched.
    #[serde(default)]
    pub archived: bool,
    pub updated_by: Id,
    pub updated_at: TimestampMicros,
}

impl ServiceDefinition {
    /// A service nobody has described yet: no owner, no tags, nothing starred.
    pub fn blank(namespace: &str, name: &str, updated_by: Id, updated_at: TimestampMicros) -> Self {
        Self {
            namespace: namespace.to_owned(),
            name: name.to_owned(),
            description: String::new(),
            team_id: None,
            owner_id: None,
            tier: None,
            lifecycle: None,
            repository_url: None,
            tags: BTreeMap::new(),
            apdex_threshold: None,
            key_transactions: Vec::new(),
            archived: false,
            updated_by,
            updated_at,
        }
    }

    /// Stars `transaction` as a key transaction, or takes the star off; `false`
    /// when it already was as asked.
    pub fn mark_key_transaction(&mut self, transaction: &TransactionIdentity, key: bool) -> bool {
        let held = self.key_transactions.contains(transaction);

        match (key, held) {
            (true, false) => self.key_transactions.push(transaction.clone()),
            (false, true) => self.key_transactions.retain(|kept| kept != transaction),
            _ => return false,
        }

        true
    }

    pub fn validate(&self) -> Result<()> {
        if self.namespace.trim().is_empty() || self.name.trim().is_empty() {
            return Err(Error::invalid(
                "a service definition needs a namespace and a name",
            ));
        }

        if self.namespace.chars().count() > 128 || self.name.chars().count() > 255 {
            return Err(Error::invalid("the service namespace or name is too long"));
        }

        if self.description.chars().count() > SERVICE_DESCRIPTION_MAX_LENGTH {
            return Err(Error::invalid(format!(
                "a service description is at most {SERVICE_DESCRIPTION_MAX_LENGTH} characters"
            )));
        }

        // Kept masked: what is written is masked before it gets here.
        if let Some(address) = &self.repository_url
            && masked_repository_address(address).as_deref() != Some(address.as_str())
        {
            return Err(Error::invalid(
                "a repository is an http, https, ssh or git address, or user@host:path, without spaces and with its password masked",
            ));
        }

        validate_tags(&self.tags)?;

        if self
            .apdex_threshold
            .is_some_and(|micros| micros == 0 || micros > SERVICE_APDEX_THRESHOLD_MAX_MICROS)
        {
            return Err(Error::invalid(
                "an Apdex threshold is more than 0 and at most a minute",
            ));
        }

        validate_key_transactions(&self.key_transactions)
    }
}

/// A tag's key is lowercase letters, digits and `_ . - /`; its value any
/// printable text, the key alone when empty.
fn validate_tags(tags: &BTreeMap<String, String>) -> Result<()> {
    if tags.len() > SERVICE_TAGS_MAX {
        return Err(Error::invalid(format!(
            "a service has at most {SERVICE_TAGS_MAX} tags"
        )));
    }

    for (key, value) in tags {
        let key_shaped = !key.is_empty()
            && key.chars().count() <= SERVICE_TAG_KEY_MAX_LENGTH
            && key.chars().all(|char| {
                char.is_ascii_lowercase() || char.is_ascii_digit() || "_.-/".contains(char)
            });

        if !key_shaped {
            return Err(Error::invalid(format!(
                "a tag key is 1 to {SERVICE_TAG_KEY_MAX_LENGTH} lowercase letters, digits, _ . - or /: {key}"
            )));
        }

        if value.chars().count() > SERVICE_TAG_VALUE_MAX_LENGTH
            || value.trim() != value
            || value.chars().any(char::is_control)
        {
            return Err(Error::invalid(format!(
                "the value of tag {key} is at most {SERVICE_TAG_VALUE_MAX_LENGTH} characters of text, without spaces around it"
            )));
        }
    }

    Ok(())
}

fn validate_key_transactions(transactions: &[TransactionIdentity]) -> Result<()> {
    if transactions.len() > SERVICE_KEY_TRANSACTIONS_MAX {
        return Err(Error::invalid(format!(
            "a service has at most {SERVICE_KEY_TRANSACTIONS_MAX} key transactions"
        )));
    }

    for (index, transaction) in transactions.iter().enumerate() {
        if transaction.name.trim().is_empty()
            || transaction.name.len() > 512
            || transaction.name == OTHER_DIMENSION
        {
            return Err(Error::invalid(
                "a key transaction is an endpoint of the service, named in 1 to 512 bytes",
            ));
        }

        if transactions[..index].contains(transaction) {
            return Err(Error::invalid(format!(
                "{} is a key transaction twice",
                transaction.name
            )));
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorIssueStatus {
    Open,
    /// Someone is looking into it: still unresolved.
    Investigating,
    Resolved,
    Ignored,
}

impl ErrorIssueStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Investigating => "investigating",
            Self::Resolved => "resolved",
            Self::Ignored => "ignored",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "open" => Some(Self::Open),
            "investigating" => Some(Self::Investigating),
            "resolved" => Some(Self::Resolved),
            "ignored" => Some(Self::Ignored),
            _ => None,
        }
    }
}

/// What is being done about an error group. A group nobody has touched has
/// none and reads as open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorIssue {
    pub fingerprint: String,
    pub status: ErrorIssueStatus,
    pub assignee_id: Option<Id>,
    /// The platform team that has it.
    #[serde(default)]
    pub team_id: Option<Id>,
    /// When it was last marked resolved; an occurrence after it is a regression.
    pub resolved_at: Option<TimestampMicros>,
    pub updated_by: Id,
    pub updated_at: TimestampMicros,
}

impl ErrorIssue {
    /// Resolved, and seen again since.
    pub fn regressed(&self, last_seen_at: TimestampMicros) -> bool {
        self.status == ErrorIssueStatus::Resolved
            && self
                .resolved_at
                .is_some_and(|resolved| last_seen_at > resolved)
    }
}

/// An endpoint of a service starred as a key transaction, or its star taken off.
#[derive(Debug, Clone)]
pub struct KeyTransactionMark {
    pub namespace: String,
    pub name: String,
    pub transaction: TransactionIdentity,
    pub key: bool,
    pub updated_by: Id,
    pub updated_at: TimestampMicros,
}

#[async_trait]
pub trait ApmCatalogRepository: Send + Sync {
    async fn list_definitions(&self, org_id: &Id) -> Result<Vec<ServiceDefinition>>;

    /// Writes the definition, but for its key transactions: a definition
    /// already kept keeps its own, which change one at a time by
    /// [`Self::mark_key_transaction`].
    async fn save_definition(&self, org_id: &Id, definition: &ServiceDefinition) -> Result<()>;

    /// Stars an endpoint or takes its star off, describing the service when
    /// nobody has yet; the definition as it is after.
    async fn mark_key_transaction(
        &self,
        org_id: &Id,
        mark: &KeyTransactionMark,
    ) -> Result<ServiceDefinition>;

    /// Whether there was one to delete.
    async fn delete_definition(&self, org_id: &Id, namespace: &str, name: &str) -> Result<bool>;

    async fn list_issues(&self, org_id: &Id, fingerprints: &[String]) -> Result<Vec<ErrorIssue>>;

    /// `false` when the error group is not (or no longer) known.
    async fn save_issue(&self, org_id: &Id, issue: &ErrorIssue) -> Result<bool>;

    async fn list_slos(&self, org_id: &Id) -> Result<Vec<super::ServiceLevelObjective>>;

    async fn get_slo(&self, org_id: &Id, id: &Id) -> Result<Option<super::ServiceLevelObjective>>;

    /// Creates or replaces the objective with its id.
    async fn save_slo(&self, org_id: &Id, slo: &super::ServiceLevelObjective) -> Result<()>;

    /// Whether there was one to delete.
    async fn delete_slo(&self, org_id: &Id, id: &Id) -> Result<bool>;

    /// The organization's error rules in the order they are tried.
    async fn list_error_rules(&self, org_id: &Id) -> Result<Vec<super::ErrorRule>>;

    /// Every organization's error rules, for where errors are fingerprinted.
    async fn list_all_error_rules(&self) -> Result<Vec<(Id, super::ErrorRule)>>;

    /// Creates or replaces the rule with its id.
    async fn save_error_rule(&self, org_id: &Id, rule: &super::ErrorRule) -> Result<()>;

    /// Whether there was one to delete.
    async fn delete_error_rule(&self, org_id: &Id, id: &Id) -> Result<bool>;

    /// Puts the rules named in that order, before those not named.
    async fn order_error_rules(&self, org_id: &Id, ids: &[Id]) -> Result<()>;

    async fn save_deployment(&self, org_id: &Id, event: &super::DeploymentEvent) -> Result<()>;

    /// The organization's deployments the query keeps, the latest first.
    async fn list_deployments(
        &self,
        org_id: &Id,
        query: &super::DeploymentQuery,
    ) -> Result<Vec<super::DeploymentEvent>>;

    /// The organization's APM settings; `None` while it has set none.
    async fn get_settings(&self, org_id: &Id) -> Result<Option<super::ApmOrgSettings>>;

    async fn save_settings(
        &self,
        org_id: &Id,
        settings: &super::ApmOrgSettings,
        updated_by: &Id,
    ) -> Result<()>;

    /// Every organization's APM settings, for where spans are read.
    async fn list_all_settings(&self) -> Result<Vec<(Id, super::ApmOrgSettings)>>;
}
