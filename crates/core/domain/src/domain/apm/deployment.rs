// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Changes as the pipelines and tools that made them report them: a version
//! of a service going out to an environment, how it went, and where it came
//! from (commit, repository, pull request, author, the CI run); or a change
//! of the service's configuration or of a feature flag. The versions APM
//! sees in spans are told apart from these; the deployments page puts the two
//! side by side, and insights name the changes near a move.

use serde::{Deserialize, Serialize};

use super::masked_repository_address;
use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

/// The most deployments a list answers.
pub const DEPLOYMENTS_LISTED_MAX: usize = 200;

/// What changed: a version deployed, the configuration, or a feature flag.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    #[default]
    Deployment,
    Configuration,
    FeatureFlag,
}

impl ChangeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Deployment => "deployment",
            Self::Configuration => "configuration",
            Self::FeatureFlag => "feature_flag",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "deployment" => Some(Self::Deployment),
            "configuration" => Some(Self::Configuration),
            "feature_flag" => Some(Self::FeatureFlag),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentStatus {
    Succeeded,
    Failed,
    /// The version was taken back out.
    RolledBack,
}

impl DeploymentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::RolledBack => "rolled_back",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "succeeded" => Some(Self::Succeeded),
            "failed" => Some(Self::Failed),
            "rolled_back" => Some(Self::RolledBack),
            _ => None,
        }
    }
}

/// A deployment, or another change. A namespace or environment left empty
/// was not said by the pipeline: the change is listed in every one. A change
/// other than a deployment may name no version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentEvent {
    pub id: Id,
    #[serde(default)]
    pub kind: ChangeKind,
    pub namespace: String,
    pub service: String,
    pub environment: String,
    pub version: String,
    pub status: DeploymentStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// The code's repository, its credentials masked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull_request_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ci_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub deployed_at: TimestampMicros,
    pub created_by: Id,
    pub created_at: TimestampMicros,
}

impl DeploymentEvent {
    pub fn validate(&self) -> Result<()> {
        let required = [
            ("service", self.service.as_str(), 255, true),
            (
                "version",
                self.version.as_str(),
                192,
                self.kind == ChangeKind::Deployment,
            ),
        ];

        for (field, value, limit, needed) in required {
            if (needed && value.trim().is_empty()) || value.chars().count() > limit {
                return Err(Error::invalid(format!(
                    "a {} needs a {field} of at most {limit} characters",
                    self.kind.as_str().replace('_', " ")
                )));
            }
        }

        if self.namespace.chars().count() > 128 || self.environment.chars().count() > 128 {
            return Err(Error::invalid(
                "a deployment's namespace and environment have at most 128 characters",
            ));
        }

        if let Some(commit) = &self.commit
            && (commit.is_empty()
                || commit.len() > 64
                || !commit.chars().all(|char| char.is_ascii_alphanumeric()))
        {
            return Err(Error::invalid("a commit is at most 64 letters and digits"));
        }

        if let Some(address) = &self.repository_url
            && (address.chars().count() > 512
                || masked_repository_address(address).as_ref() != Some(address))
        {
            return Err(Error::invalid(
                "a repository is an https, http, ssh or git address of at most 512 characters, \
                 its password masked",
            ));
        }

        for (field, link) in [
            ("pull_request_url", &self.pull_request_url),
            ("ci_url", &self.ci_url),
        ] {
            if let Some(link) = link
                && !web_link(link)
            {
                return Err(Error::invalid(format!(
                    "{field} is an http(s) address of at most 512 characters"
                )));
            }
        }

        if self
            .author
            .as_ref()
            .is_some_and(|author| author.chars().count() > 128)
            || self
                .description
                .as_ref()
                .is_some_and(|description| description.chars().count() > 1_000)
        {
            return Err(Error::invalid(
                "an author has at most 128 characters, a description at most 1000",
            ));
        }

        Ok(())
    }
}

fn web_link(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();

    value.chars().count() <= 512
        && (lower.starts_with("https://") || lower.starts_with("http://"))
        && !value.contains(char::is_whitespace)
}

/// Which deployments a list keeps.
#[derive(Debug, Clone, Default)]
pub struct DeploymentQuery {
    pub namespace: Option<String>,
    pub service: Option<String>,
    pub environment: Option<String>,
    pub from: Option<TimestampMicros>,
    pub to: Option<TimestampMicros>,
    pub limit: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event() -> DeploymentEvent {
        DeploymentEvent {
            id: Id::from_string("deploy-1"),
            kind: ChangeKind::Deployment,
            namespace: "shop".into(),
            service: "checkout".into(),
            environment: "prod".into(),
            version: "2.0.0".into(),
            status: DeploymentStatus::Succeeded,
            commit: Some("9f2c1e7a4b3d".into()),
            repository_url: Some("https://github.com/acme/checkout".into()),
            pull_request_url: Some("https://github.com/acme/checkout/pull/42".into()),
            author: Some("ada".into()),
            ci_url: Some("https://ci.example.com/runs/7".into()),
            description: None,
            deployed_at: TimestampMicros(1),
            created_by: Id::from_string("token-1"),
            created_at: TimestampMicros(1),
        }
    }

    #[test]
    fn a_deployment_says_what_went_where_and_links_where_it_came_from() {
        assert!(event().validate().is_ok());

        let unnamed = DeploymentEvent {
            version: " ".into(),
            ..event()
        };
        // A configuration change names no version.
        let configuration = DeploymentEvent {
            kind: ChangeKind::Configuration,
            version: String::new(),
            description: Some("checkout.timeout_ms 2000 → 500".into()),
            ..event()
        };
        let odd_commit = DeploymentEvent {
            commit: Some("not a sha".into()),
            ..event()
        };
        let not_a_link = DeploymentEvent {
            ci_url: Some("javascript:alert(1)".into()),
            ..event()
        };
        let password = DeploymentEvent {
            repository_url: Some("https://ada:secret@github.com/acme/checkout".into()),
            ..event()
        };

        assert!(unnamed.validate().is_err());
        assert!(configuration.validate().is_ok());
        assert!(odd_commit.validate().is_err());
        assert!(not_a_link.validate().is_err());
        assert!(password.validate().is_err());
        assert_eq!(
            DeploymentStatus::parse("rolled_back"),
            Some(DeploymentStatus::RolledBack)
        );
    }
}
