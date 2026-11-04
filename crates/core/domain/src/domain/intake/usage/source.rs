// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! How much each source has taken in: hourly rows that say who sent the data, into which stream,
//! and for which service. The credential, the stream and the service are three views of the same
//! bytes, so that their totals agree.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    domain::{
        iam::IamContext,
        stream::{StreamType, validate_stream_name},
    },
    shared::{Error, Result, ids::Id},
};

pub const HOUR_MICROS: i64 = 3_600 * 1_000_000;

/// The service of events that name none.
pub const UNNAMED_SERVICE: &str = "";
/// The services past the limit of an organization and hour, counted together.
pub const OTHER_SERVICES: &str = "*";
/// The most characters of a service name that are kept.
pub const MAX_SERVICE_NAME_CHARS: usize = 128;
/// Where session replays are counted. They are kept as files, in no stream of their own.
pub const RUM_REPLAY_SOURCE: &str = "rum_replay";

/// What a credential is: tokens, push and pull connectors, signed-in sessions, the platform itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    ApiToken,
    Connector,
    User,
    Internal,
}

impl CredentialKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ApiToken => "api_token",
            Self::Connector => "connector",
            Self::User => "user",
            Self::Internal => "internal",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "api_token" => Ok(Self::ApiToken),
            "connector" => Ok(Self::Connector),
            "user" => Ok(Self::User),
            "internal" => Ok(Self::Internal),
            other => Err(Error::invalid(format!("unknown credential kind `{other}`"))),
        }
    }
}

/// Who sent the data of a request.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IntakeCredential {
    pub kind: CredentialKind,
    pub id: String,
}

impl IntakeCredential {
    /// The token the request authenticated with, or else the signed-in user.
    pub fn of(ctx: &IamContext) -> Self {
        match &ctx.credential_token_id {
            Some(token_id) => Self {
                kind: CredentialKind::ApiToken,
                id: token_id.to_string(),
            },
            None => Self {
                kind: CredentialKind::User,
                id: ctx.user_id.to_string(),
            },
        }
    }

    pub fn connector(connector_id: &Id) -> Self {
        Self {
            kind: CredentialKind::Connector,
            id: connector_id.to_string(),
        }
    }

    pub fn internal() -> Self {
        Self {
            kind: CredentialKind::Internal,
            id: String::new(),
        }
    }

    pub fn source_key(&self) -> String {
        format!("{}:{}", self.kind.as_str(), self.id)
    }
}

/// A way to divide what was taken in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageDimension {
    Credential,
    Stream,
    Service,
}

impl UsageDimension {
    pub const ALL: [Self; 3] = [Self::Credential, Self::Stream, Self::Service];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Credential => "credential",
            Self::Stream => "stream",
            Self::Service => "service",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "credential" => Ok(Self::Credential),
            "stream" => Ok(Self::Stream),
            "service" => Ok(Self::Service),
            other => Err(Error::invalid(format!("unknown usage dimension `{other}`"))),
        }
    }
}

/// `<stream_type>:<name>`, the key of a stream among the sources.
pub fn stream_source_key(stream_type: StreamType, stream: &str) -> String {
    format!("{}:{stream}", stream_type.external_name())
}

/// The kind and id of a credential's source key.
pub fn parse_credential_key(key: &str) -> Result<(CredentialKind, &str)> {
    let (kind, id) = key
        .split_once(':')
        .ok_or_else(|| Error::invalid(format!("credential source `{key}` has no kind")))?;

    Ok((CredentialKind::parse(kind)?, id))
}

/// The type and name of a stream's source key.
pub fn parse_stream_key(key: &str) -> Result<(StreamType, &str)> {
    let (stream_type, stream) = key
        .split_once(':')
        .ok_or_else(|| Error::invalid(format!("stream source `{key}` has no type")))?;

    Ok((StreamType::from_external_name(stream_type)?, stream))
}

/// Whether a budget can be set on this source: anything that can be told apart and answers for
/// its own data. The platform's own telemetry, events that name no service and the services past
/// the limit cannot.
pub fn budgetable(dimension: UsageDimension, key: &str) -> bool {
    match dimension {
        UsageDimension::Credential => parse_credential_key(key).is_ok_and(|(kind, id)| {
            kind != CredentialKind::Internal && !id.is_empty() && id.len() <= 64
        }),
        UsageDimension::Stream => {
            parse_stream_key(key).is_ok_and(|(_, stream)| validate_stream_name(stream).is_ok())
        }
        UsageDimension::Service => {
            key != UNNAMED_SERVICE
                && key != OTHER_SERVICES
                && key.chars().count() <= MAX_SERVICE_NAME_CHARS
                && !key.chars().any(char::is_control)
        }
    }
}

pub fn hour_bucket_start(timestamp_micros: i64) -> i64 {
    timestamp_micros.div_euclid(HOUR_MICROS) * HOUR_MICROS
}

/// One row of hourly usage, or what is to be added to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntakeUsageRow {
    pub org_id: Id,
    pub bucket_start_micros: i64,
    pub credential: IntakeCredential,
    pub stream_type: StreamType,
    pub stream: String,
    pub service: String,
    pub intake_bytes: u64,
    pub events: u64,
}

/// What was taken in during one hour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntakeUsageBucket {
    pub bucket_start_micros: i64,
    pub intake_bytes: u64,
    pub events: u64,
}

/// What one source took in during a span of time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceUsage {
    pub source_key: String,
    pub intake_bytes: u64,
    pub events: u64,
}

/// The rows of one organization and span of hours, or only those of one source.
#[derive(Debug, Clone, Copy)]
pub struct UsageSelection<'a> {
    pub org_id: &'a Id,
    pub start_micros: i64,
    pub end_micros: i64,
    pub source: Option<(UsageDimension, &'a str)>,
}

#[async_trait]
pub trait IntakeUsageRepository: Send + Sync {
    /// Adds the rows to what is already kept for their hours.
    async fn add(&self, rows: &[IntakeUsageRow]) -> Result<()>;

    /// What each source of `dimension` took in, by the hours that meet the span.
    async fn by_source(
        &self,
        dimension: UsageDimension,
        selection: UsageSelection<'_>,
    ) -> Result<Vec<SourceUsage>>;

    /// What was taken in hour by hour, oldest first.
    async fn hourly(&self, selection: UsageSelection<'_>) -> Result<Vec<IntakeUsageBucket>>;

    /// The start of the oldest hour kept for the organization. What is older is not known.
    async fn first_bucket(&self, org_id: &Id) -> Result<Option<i64>>;

    /// Deletes at most `limit` rows older than `before_micros`, across organizations.
    async fn delete_before(&self, before_micros: i64, limit: i64) -> Result<u64>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hour_bucket_start_uses_utc_epoch_boundaries() {
        assert_eq!(hour_bucket_start(0), 0);
        assert_eq!(hour_bucket_start(HOUR_MICROS - 1), 0);
        assert_eq!(hour_bucket_start(HOUR_MICROS), HOUR_MICROS);
        assert_eq!(hour_bucket_start(HOUR_MICROS + 42), HOUR_MICROS);
    }

    #[test]
    fn source_keys_read_back() {
        let credential = IntakeCredential::connector(&Id::from_string("c-1"));
        let key = credential.source_key();

        assert_eq!(key, "connector:c-1");
        assert_eq!(
            parse_credential_key(&key).unwrap(),
            (CredentialKind::Connector, "c-1")
        );

        let key = stream_source_key(StreamType::LOGS, "app_logs");
        let (stream_type, stream) = parse_stream_key(&key).unwrap();

        assert_eq!(stream_type, StreamType::LOGS);
        assert_eq!(stream, "app_logs");
        assert!(parse_credential_key("c-1").is_err());
        assert!(parse_stream_key("app_logs").is_err());
    }

    #[test]
    fn only_a_source_that_answers_for_its_own_data_takes_a_budget() {
        use UsageDimension::{Credential, Service, Stream};

        assert!(budgetable(Credential, "api_token:t-1"));
        assert!(budgetable(Credential, "connector:c-1"));
        assert!(budgetable(Credential, "user:u-1"));
        assert!(!budgetable(Credential, "internal:"));
        assert!(!budgetable(Credential, "api_token:"));
        assert!(!budgetable(Credential, "nonsense:t-1"));

        assert!(budgetable(
            Stream,
            &stream_source_key(StreamType::LOGS, "app_logs")
        ));
        assert!(!budgetable(Stream, "logs:"));
        assert!(!budgetable(Stream, "app_logs"));

        assert!(budgetable(Service, "checkout"));
        assert!(!budgetable(Service, UNNAMED_SERVICE));
        assert!(!budgetable(Service, OTHER_SERVICES));
        assert!(!budgetable(
            Service,
            &"x".repeat(MAX_SERVICE_NAME_CHARS + 1)
        ));
        assert!(!budgetable(Service, "bad\nname"));
    }
}
