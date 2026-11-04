// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Receiving RUM events.
//!
//! A request carries events of the shapes the contract in `contracts/rum/events/` describes. Each
//! is checked on its own, bound to an application, flattened into the row of the stream its type
//! belongs to and stamped with what the connection says (the place of the client, its browser).
//! Rows of one request are then written stream by stream; the views also go to `rum_sessions` as
//! the patches the sessions are built from.
//!
//! What an application says about its data is done here, before anything is written: the share of
//! sessions it keeps (a dropped session leaves no event behind, but for its errors when it keeps
//! those) and the sensitive things it takes out of the text of an event. Every row says the share
//! of such rows that is kept, so that totals can be made of what is left.

mod address;
mod convert;
mod decode;
mod enrich;
mod flatten;
mod retention;
mod sample;
mod scrub;
mod split;
mod streams;
mod validate;

use std::{net::IpAddr, sync::Arc, time::Duration};

use moka::future::Cache;
use serde::Serialize;
use serde_json::{Map, Value, json};

pub use self::{
    decode::{Decoded, DecodedBody, MAX_DECODED_BYTES, MAX_EVENTS_PER_REQUEST, decode_body},
    enrich::{AgentInfo, Enricher, Origin},
    streams::StreamProvisioner,
};
use self::{
    retention::{Retention, Verdict},
    split::{Row, StreamRows},
};
use super::issues::{VersionLedger, identity::issue_id};
use crate::{
    domain::{
        intake::{IntakeBatch, RawEvent},
        rum::{
            DEFAULT_FILTER, RumApplication, RumApplicationRepository, RumApplicationSettings,
            RumEventWriter, RumVersionRepository, event::RumStream,
        },
        stream::{StreamRepository, StreamType},
    },
    infra::enrichment::GeoLookup,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// How long an application is remembered, whether it exists or not.
const APPLICATION_TTL: Duration = Duration::from_secs(30);

/// What a request asks for.
pub struct Request<'a> {
    pub org_id: &'a Id,
    /// The application the credential is bound to. An event can then only be written to it.
    /// A credential bound to none leaves each event to name its application.
    pub bound_application: Option<&'a str>,
    pub events: Vec<Decoded>,
    pub origin: Origin,
}

/// An event that was not written, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rejected {
    /// Where the event stood in the request.
    pub index: usize,
    pub reason: String,
}

/// What became of a request.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    pub accepted: usize,
    pub rejected: Vec<Rejected>,
    /// Events of sessions the application does not keep, which were left out on purpose. It is
    /// not said when there are none.
    #[serde(skip_serializing_if = "is_zero")]
    pub sampled: usize,
}

fn is_zero(count: &usize) -> bool {
    *count == 0
}

/// What an event of a request came to.
enum Prepared {
    /// The row it is written as.
    Row(RumStream, RawEvent),
    /// The reason it is refused.
    Refused(String),
    /// It belongs to a session the application does not keep.
    Sampled,
}

pub struct RumIntake {
    writer: Arc<dyn RumEventWriter>,
    applications: Arc<dyn RumApplicationRepository>,
    streams: StreamProvisioner,
    enricher: Enricher,
    known: Cache<(Id, String), Option<Arc<RumApplication>>>,
    /// Where the versions of the applications are kept in the order they were first seen. Without
    /// it, which version is newer is not known, and a resolved issue is reopened by time alone.
    versions: Option<VersionLedger>,
}

impl RumIntake {
    pub fn new(
        writer: Arc<dyn RumEventWriter>,
        streams: Arc<dyn StreamRepository>,
        applications: Arc<dyn RumApplicationRepository>,
        geo: Arc<dyn GeoLookup>,
    ) -> Self {
        Self {
            writer,
            applications,
            streams: StreamProvisioner::new(streams),
            enricher: Enricher::new(geo),
            known: Cache::builder()
                .max_capacity(10_000)
                .time_to_live(APPLICATION_TTL)
                .build(),
            versions: None,
        }
    }

    /// Keeps the versions of the applications as events are received.
    pub fn with_versions(mut self, repository: Arc<dyn RumVersionRepository>) -> Self {
        self.versions = Some(VersionLedger::new(repository));
        self
    }

    /// The client of a request, looked up once for all its events.
    pub fn origin(&self, ip: Option<IpAddr>, user_agent: Option<&str>) -> Origin {
        self.enricher.origin(ip, user_agent)
    }

    /// Creates the streams of an organization ahead of its first event, so that pages read empty
    /// streams instead of missing ones.
    pub async fn provision_streams(&self, org_id: &Id) -> Result<()> {
        self.streams.provision(org_id).await
    }

    /// Drops what is remembered of an application after it was changed or deleted here.
    pub async fn forget(&self, org_id: &Id, application_id: &str) {
        self.known
            .invalidate(&(org_id.clone(), application_id.to_string()))
            .await;
    }

    /// Whether a replay segment of a session is kept. Of the session only what the request shows is
    /// known, and what the SDK says of it in `said`: the filters that look at more than that keep
    /// the segment if they might keep the session.
    pub fn keeps_replay(
        &self,
        settings: &RumApplicationSettings,
        session_id: &str,
        said: Map<String, Value>,
        origin: &Origin,
    ) -> bool {
        let mut known = said;

        origin.stamp(&mut known, settings);

        Retention::of(settings).keeps_replay(session_id, &known)
    }

    pub async fn intake(&self, request: Request<'_>) -> Result<Report> {
        let Request {
            org_id,
            bound_application,
            events,
            origin,
        } = request;

        let now_ms = TimestampMicros::now().0 / 1000;
        let mut report = Report::default();
        let mut rows = Vec::with_capacity(events.len());

        for (index, decoded) in events.into_iter().enumerate() {
            let prepared = self
                .prepare(org_id, bound_application, decoded, now_ms, &origin)
                .await?;

            match prepared {
                Prepared::Row(stream, event) => rows.push(Row {
                    index,
                    stream,
                    event,
                }),
                Prepared::Refused(reason) => report.rejected.push(Rejected { index, reason }),
                Prepared::Sampled => report.sampled += 1,
            }
        }

        let prepared = rows.len();

        if prepared == 0 {
            return Ok(report);
        }

        if let Some(ledger) = &self.versions {
            let seen = rows.iter().filter_map(|row| {
                Some((
                    row.event.fields.get("application.id")?.as_str()?,
                    row.event.fields.get("version")?.as_str()?,
                ))
            });

            ledger.note(org_id, seen, TimestampMicros::now()).await;
        }

        self.streams.provision(org_id).await?;

        let received_at = TimestampMicros::now();

        let writes = split::assemble(rows)
            .into_iter()
            .map(|rows| self.write(org_id, rows, received_at));

        let mut refused = Vec::new();

        for outcome in futures::future::join_all(writes).await {
            refused.extend(outcome?);
        }

        let mut refused_events = refused.iter().map(|item| item.index).collect::<Vec<_>>();

        refused_events.sort_unstable();
        refused_events.dedup();

        report.accepted = prepared - refused_events.len();
        report.rejected.extend(refused);
        report.rejected.sort_by_key(|item| item.index);

        Ok(report)
    }

    /// One event of the request as the row it is written as, or the reason it is refused, or that
    /// it is left out with the session it belongs to. A failure to look an application up is not
    /// a reason to refuse the event: it fails the request.
    async fn prepare(
        &self,
        org_id: &Id,
        bound_application: Option<&str>,
        decoded: Decoded,
        now_ms: i64,
        origin: &Origin,
    ) -> Result<Prepared> {
        let value = match decoded {
            Ok(value) => value,
            Err(reason) => return Ok(Prepared::Refused(format!("line is not JSON: {reason}"))),
        };

        let mut validated = match validate::validate(value, now_ms) {
            Ok(validated) => validated,
            Err(reason) => return Ok(Prepared::Refused(reason)),
        };

        let application_id = match (bound_application, validated.application.as_deref()) {
            (Some(bound), Some(named)) if named != bound => {
                return Ok(Prepared::Refused(
                    "application.id is not the application of the client token".into(),
                ));
            }
            (Some(bound), _) => bound.to_string(),
            (None, Some(named)) => named.to_string(),
            (None, None) => return Ok(Prepared::Refused("application.id is required".into())),
        };

        let Some(application) = self.application(org_id, &application_id).await? else {
            return Ok(Prepared::Refused(format!(
                "application `{application_id}` does not exist"
            )));
        };

        let stream = validated.kind.stream();
        let error = stream == RumStream::Errors;
        let retention = Retention::of(&application.settings);

        let session = validated
            .event
            .get("session")
            .and_then(|session| session.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string);

        // A session no person ran counts for nothing of the application's: it is kept to be found
        // from the check that made it, and its errors open no issue.
        let tested = validated
            .event
            .get("session")
            .and_then(|session| session.get("type"))
            .and_then(Value::as_str)
            .is_some_and(|kind| kind != "user");

        // A session is kept whole or not at all, and which share it is kept with depends on what
        // it is like. What no share of the application would keep is left out before anything is
        // spent on the event; the rest is judged when the event is a row.
        if let Some(session) = &session
            && !retention.may_keep(session, error, tested)
        {
            return Ok(Prepared::Sampled);
        }

        // The text goes before anything is made of it: the addresses are grouped, and the issue
        // of an error is worked out, from what is kept.
        scrub::apply(&application.settings.scrub, &mut validated.event);

        validated
            .event
            .insert("application".into(), json!({ "id": application_id }));

        let mut row = flatten::flatten(stream, validated.event);

        origin.stamp(&mut row, &application.settings);

        // What the session is like is known now, with the place and the browser. A row says what
        // it was kept by, whatever an SDK put in its place.
        let (rate, filter) = match &session {
            Some(session) => match retention.judge(session, &row, error) {
                Verdict::Kept { rate, filter } => (rate, filter),
                Verdict::Dropped => return Ok(Prepared::Sampled),
            },
            None => (100, DEFAULT_FILTER.to_string()),
        };

        row.insert("retention.rate".into(), Value::from(f64::from(rate)));
        row.insert("retention.filter".into(), Value::String(filter));

        // The issue an error belongs to is worked out here, from the error alone, so that every
        // node agrees; the tracker settles the rest where the errors are written.
        if stream == RumStream::Errors {
            row.remove("error.issue_id");

            if !tested && let Some(id) = issue_id(&row) {
                row.insert("error.issue_id".into(), Value::String(id));
            }
        }

        let event = RawEvent {
            timestamp: TimestampMicros(validated.date_ms.saturating_mul(1000)),
            fields: row,
        };

        Ok(Prepared::Row(stream, event))
    }

    async fn application(&self, org_id: &Id, id: &str) -> Result<Option<Arc<RumApplication>>> {
        self.known
            .try_get_with((org_id.clone(), id.to_string()), async {
                match self.applications.get(org_id, id).await {
                    Ok(application) => Ok(Some(Arc::new(application))),
                    Err(Error::NotFound(_)) => Ok(None),
                    Err(error) => Err(error),
                }
            })
            .await
            .map_err(|error| Error::unavailable(format!("RUM application lookup failed: {error}")))
    }

    async fn write(
        &self,
        org_id: &Id,
        rows: StreamRows,
        received_at: TimestampMicros,
    ) -> Result<Vec<Rejected>> {
        let StreamRows { stream, rows } = rows;
        let (indexes, events): (Vec<usize>, Vec<RawEvent>) = rows.into_iter().unzip();

        let batch = IntakeBatch {
            batch_id: Id::new(),
            org_id: org_id.clone(),
            stream: stream.name().to_string(),
            stream_type: StreamType::LOGS,
            events,
            received_at,
        };

        let result = self.writer.write(batch).await?;

        Ok(result
            .errors
            .into_iter()
            .map(|error| Rejected {
                index: indexes.get(error.index).copied().unwrap_or(error.index),
                reason: format!("{}: {}", stream.name(), error.reason),
            })
            .collect())
    }
}

#[cfg(test)]
mod fit;
#[cfg(test)]
mod tests;
