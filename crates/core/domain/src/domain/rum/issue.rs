// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Issues: the errors of an application that are alike, kept as one thing people can act on. An
//! issue has a state, a team and a person who has it, and a record of when and in which versions it
//! was seen. The errors stay in `rum_errors`, each with the id of the issue it belongs to.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

/// Longest message kept with an issue, in bytes.
pub const MAX_MESSAGE_LEN: usize = 512;
/// Longest place (file, line and function) kept with an issue, in bytes.
pub const MAX_LOCATION_LEN: usize = 512;
/// Longest version kept, in bytes.
pub const MAX_VERSION_LEN: usize = 128;
/// Longest environment kept, in bytes.
pub const MAX_ENVIRONMENT_LEN: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RumIssueStatus {
    /// Nobody has looked at it yet.
    Open,
    /// Someone is looking into it; still unresolved.
    Investigating,
    /// Resolved, and seen again in a version newer than the one it was resolved in. Only the
    /// rules set it: a person cannot.
    Regressed,
    Resolved,
    Ignored,
}

impl RumIssueStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Investigating => "investigating",
            Self::Regressed => "regressed",
            Self::Resolved => "resolved",
            Self::Ignored => "ignored",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "open" => Some(Self::Open),
            "investigating" => Some(Self::Investigating),
            "regressed" => Some(Self::Regressed),
            "resolved" => Some(Self::Resolved),
            "ignored" => Some(Self::Ignored),
            _ => None,
        }
    }

    /// Whether the issue still needs somebody: not resolved, not set aside.
    pub fn is_unresolved(self) -> bool {
        matches!(self, Self::Open | Self::Investigating | Self::Regressed)
    }
}

/// An issue as kept. What it is comes from the error that opened it; what people do about it
/// from them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RumIssue {
    /// A hash of what the errors of the issue have in common, 16 hexadecimal digits.
    pub id: String,
    pub application_id: String,
    pub status: RumIssueStatus,
    pub assignee_id: Option<Id>,
    /// The platform team that has it.
    pub team_id: Option<Id>,
    /// The errors of an application past the issues it may open in an hour, counted together.
    pub overflow: bool,
    /// Where the error comes from: `source`, `network`, `custom`…
    pub source: String,
    pub error_type: String,
    pub message: String,
    /// The first frame of the application's own code, as `file:line · function`.
    pub location: String,
    pub first_seen_at: TimestampMicros,
    pub last_seen_at: TimestampMicros,
    pub first_version: Option<String>,
    pub last_version: Option<String>,
    pub resolved_at: Option<TimestampMicros>,
    /// The newest version the application had when the issue was resolved.
    pub resolved_version: Option<String>,
    pub resolved_by: Option<Id>,
    pub regressed_at: Option<TimestampMicros>,
    pub regressed_version: Option<String>,
    /// The environment of the error that brought the issue back.
    pub regressed_environment: Option<String>,
    pub updated_by: Option<Id>,
    pub updated_at: TimestampMicros,
}

impl RumIssue {
    /// A new issue, open, first seen as the arguments say.
    pub fn opened(
        id: String,
        application_id: String,
        at: TimestampMicros,
        version: Option<String>,
    ) -> Self {
        Self {
            id,
            application_id,
            status: RumIssueStatus::Open,
            assignee_id: None,
            team_id: None,
            overflow: false,
            source: String::new(),
            error_type: String::new(),
            message: String::new(),
            location: String::new(),
            first_seen_at: at,
            last_seen_at: at,
            first_version: version.clone(),
            last_version: version,
            resolved_at: None,
            resolved_version: None,
            resolved_by: None,
            regressed_at: None,
            regressed_version: None,
            regressed_environment: None,
            updated_by: None,
            updated_at: at,
        }
    }

    /// Whether an error seen at `at` in `version` reopens the issue. Only a resolved issue is
    /// reopened. One that was resolved in a version is reopened by an error in a version that was
    /// first seen later, never by one in the same or an older version, which users who did not
    /// update keep meeting. With no version to go by, any error after the resolving reopens it.
    /// `first_seen` says when the application first had a version.
    pub fn is_reopened_by(
        &self,
        at: TimestampMicros,
        version: Option<&str>,
        first_seen: impl Fn(&str) -> Option<TimestampMicros>,
    ) -> bool {
        if self.status != RumIssueStatus::Resolved {
            return false;
        }

        let Some(resolved_at) = self.resolved_at else {
            return false;
        };

        match (self.resolved_version.as_deref(), version) {
            (Some(resolved), Some(seen)) => match (first_seen(resolved), first_seen(seen)) {
                (Some(resolved), Some(seen)) => seen > resolved,
                _ => false,
            },
            (Some(_), None) => false,
            (None, _) => at > resolved_at,
        }
    }
}

/// An issue seen again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RumIssueSighting {
    pub id: String,
    pub at: TimestampMicros,
    pub version: Option<String>,
}

/// An issue seen in an environment for the first time. An issue is one for the whole application;
/// this is what tells where it turned up, and when it turned up there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RumIssueEnvironment {
    pub issue_id: String,
    pub application_id: String,
    pub environment: String,
    pub first_seen_at: TimestampMicros,
}

/// An issue and the moment something happened to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RumIssueMoment {
    pub issue_id: String,
    pub at: TimestampMicros,
}

/// What a person changes of an issue. What is left out stays; a team or a person given as `None`
/// is taken off.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RumIssueChange {
    pub status: Option<RumIssueStatus>,
    pub assignee: Option<Option<Id>>,
    pub team: Option<Option<Id>>,
}

impl RumIssueChange {
    pub fn validate(&self) -> Result<()> {
        if self.status == Some(RumIssueStatus::Regressed) {
            return Err(Error::invalid(
                "`regressed` is set by the rules when a resolved issue is seen again, not by a person",
            ));
        }

        if self.status.is_none() && self.assignee.is_none() && self.team.is_none() {
            return Err(Error::invalid(
                "name a status, an assignee or a team to change",
            ));
        }

        Ok(())
    }
}

/// `text` cut to `max` bytes at a character boundary.
pub fn cut(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }

    let mut end = max;

    while !text.is_char_boundary(end) {
        end -= 1;
    }

    &text[..end]
}

/// A version as the registry keeps it: cut to its length, and none when it is empty.
pub fn version_key(version: &str) -> Option<&str> {
    let version = cut(version.trim(), MAX_VERSION_LEN);

    (!version.is_empty()).then_some(version)
}

/// An environment as the issues keep it: cut to its length, and none when it is empty.
pub fn environment_key(environment: &str) -> Option<&str> {
    let environment = cut(environment.trim(), MAX_ENVIRONMENT_LEN);

    (!environment.is_empty()).then_some(environment)
}

#[async_trait]
pub trait RumIssueRepository: Send + Sync {
    async fn get(&self, org_id: &Id, id: &str) -> Result<Option<RumIssue>>;

    /// The issues with these ids that are kept.
    async fn get_many(&self, org_id: &Id, ids: &[String]) -> Result<Vec<RumIssue>>;

    /// Opens the issues. One that is already kept stays as it is; every id given comes back as it
    /// is kept.
    async fn open(&self, org_id: &Id, issues: &[RumIssue]) -> Result<Vec<RumIssue>>;

    /// Notes that issues were seen again: the latest time and version of each.
    async fn record_sightings(&self, org_id: &Id, sightings: &[RumIssueSighting]) -> Result<()>;

    /// Notes where issues turned up. One that is known keeps the time it was first seen there.
    async fn record_environments(
        &self,
        org_id: &Id,
        environments: &[RumIssueEnvironment],
    ) -> Result<()>;

    /// Moves an issue to regressed, if it is still resolved as it was when the regression was
    /// judged. The issue after the change, or `None` when it was changed in the meantime.
    async fn mark_regressed(
        &self,
        org_id: &Id,
        id: &str,
        resolved_at: TimestampMicros,
        at: TimestampMicros,
        version: Option<&str>,
        environment: Option<&str>,
    ) -> Result<Option<RumIssue>>;

    /// The issues of an application first seen in `[from, to)`, oldest first, in an environment
    /// when one is named: for the first time there, which is not the first time anywhere. At most
    /// `limit`.
    async fn first_seen_between(
        &self,
        org_id: &Id,
        application_id: &str,
        environment: Option<&str>,
        from: TimestampMicros,
        to: TimestampMicros,
        limit: usize,
    ) -> Result<Vec<RumIssueMoment>>;

    /// The issues of an application that came back in `[from, to)`, oldest first, in an
    /// environment when one is named. An issue counts by its latest regression. At most `limit`.
    async fn regressed_between(
        &self,
        org_id: &Id,
        application_id: &str,
        environment: Option<&str>,
        from: TimestampMicros,
        to: TimestampMicros,
        limit: usize,
    ) -> Result<Vec<RumIssueMoment>>;

    /// Applies what a person changes. Resolving records when, by whom and the newest version the
    /// application has. `None` when the issue is not kept.
    async fn update(
        &self,
        org_id: &Id,
        id: &str,
        change: &RumIssueChange,
        by: &Id,
        at: TimestampMicros,
    ) -> Result<Option<RumIssue>>;
}

/// The versions an application has been seen with, in the order they were first seen: what tells a
/// newer version from an older one, whatever the versions are called.
#[async_trait]
pub trait RumVersionRepository: Send + Sync {
    /// Notes versions as seen. One that is known keeps the time it was first seen.
    async fn record(
        &self,
        org_id: &Id,
        application_id: &str,
        versions: &[String],
        at: TimestampMicros,
    ) -> Result<()>;

    /// When each of these versions was first seen; one never seen is left out.
    async fn first_seen(
        &self,
        org_id: &Id,
        application_id: &str,
        versions: &[String],
    ) -> Result<HashMap<String, TimestampMicros>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolved(version: Option<&str>) -> RumIssue {
        RumIssue {
            status: RumIssueStatus::Resolved,
            resolved_at: Some(TimestampMicros(1_000)),
            resolved_version: version.map(str::to_string),
            ..RumIssue::opened("i".into(), "app".into(), TimestampMicros(1), None)
        }
    }

    fn firsts(version: &str) -> Option<TimestampMicros> {
        match version {
            "2.2.0" => Some(TimestampMicros(100)),
            "2.3.0" => Some(TimestampMicros(200)),
            "2.4.1" => Some(TimestampMicros(300)),
            _ => None,
        }
    }

    #[test]
    fn an_error_in_a_newer_version_reopens_what_was_resolved_in_an_older_one() {
        let issue = resolved(Some("2.3.0"));

        assert!(issue.is_reopened_by(TimestampMicros(2_000), Some("2.4.1"), firsts));
    }

    #[test]
    fn an_error_in_the_same_or_an_older_version_does_not() {
        let issue = resolved(Some("2.3.0"));

        assert!(!issue.is_reopened_by(TimestampMicros(2_000), Some("2.3.0"), firsts));
        assert!(!issue.is_reopened_by(TimestampMicros(2_000), Some("2.2.0"), firsts));
    }

    #[test]
    fn a_version_the_application_never_had_is_not_taken_to_be_newer() {
        let issue = resolved(Some("2.3.0"));

        assert!(!issue.is_reopened_by(TimestampMicros(2_000), Some("9.9.9"), firsts));
        assert!(!issue.is_reopened_by(TimestampMicros(2_000), None, firsts));
    }

    #[test]
    fn without_a_version_the_time_decides() {
        let issue = resolved(None);

        assert!(issue.is_reopened_by(TimestampMicros(2_000), None, firsts));
        assert!(issue.is_reopened_by(TimestampMicros(2_000), Some("2.4.1"), firsts));
        assert!(!issue.is_reopened_by(TimestampMicros(1_000), None, firsts));
    }

    #[test]
    fn only_a_resolved_issue_is_reopened() {
        for status in [
            RumIssueStatus::Open,
            RumIssueStatus::Investigating,
            RumIssueStatus::Regressed,
            RumIssueStatus::Ignored,
        ] {
            let issue = RumIssue {
                status,
                ..resolved(None)
            };

            assert!(!issue.is_reopened_by(TimestampMicros(2_000), None, firsts));
        }
    }

    #[test]
    fn a_person_cannot_set_regressed_and_must_change_something() {
        let regressed = RumIssueChange {
            status: Some(RumIssueStatus::Regressed),
            ..Default::default()
        };
        let resolved = RumIssueChange {
            status: Some(RumIssueStatus::Resolved),
            ..Default::default()
        };
        let team = RumIssueChange {
            team: Some(None),
            ..Default::default()
        };

        assert!(regressed.validate().is_err());
        assert!(RumIssueChange::default().validate().is_err());
        assert!(resolved.validate().is_ok());
        assert!(team.validate().is_ok());
    }

    #[test]
    fn statuses_read_back_as_they_are_written() {
        for status in [
            RumIssueStatus::Open,
            RumIssueStatus::Investigating,
            RumIssueStatus::Regressed,
            RumIssueStatus::Resolved,
            RumIssueStatus::Ignored,
        ] {
            assert_eq!(RumIssueStatus::parse(status.as_str()), Some(status));
        }

        assert_eq!(RumIssueStatus::parse("closed"), None);
        assert!(RumIssueStatus::Regressed.is_unresolved());
        assert!(!RumIssueStatus::Ignored.is_unresolved());
    }

    #[test]
    fn text_is_cut_at_a_character_boundary_and_a_version_is_trimmed() {
        assert_eq!(cut("héllo", 2), "h");
        assert_eq!(cut("abc", 10), "abc");
        assert_eq!(version_key(" 2.4.1 "), Some("2.4.1"));
        assert_eq!(version_key("  "), None);
        assert_eq!(
            version_key(&"v".repeat(200)).map(str::len),
            Some(MAX_VERSION_LEN)
        );
        assert_eq!(environment_key(" prod "), Some("prod"));
        assert_eq!(environment_key("  "), None);
        assert_eq!(
            environment_key(&"e".repeat(200)).map(str::len),
            Some(MAX_ENVIRONMENT_LEN)
        );
    }
}
