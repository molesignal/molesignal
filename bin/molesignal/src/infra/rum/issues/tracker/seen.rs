// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What the errors of one batch say of the issues they belong to: when each was first and last
//! seen, in which versions and environments, and which rows are its.

use std::collections::HashMap;

use serde_json::Value;

use super::super::identity::{location, overflow_id};
use crate::{
    domain::{
        intake::RawEvent,
        rum::{MAX_MESSAGE_LEN, RumIssue, cut, environment_key, version_key},
    },
    shared::time::TimestampMicros,
};

/// The longest the type and the source of an error are kept with an issue, in bytes.
const MAX_TYPE_LEN: usize = 256;
const MAX_SOURCE_LEN: usize = 32;

/// The latest time an issue was seen in one version and one environment.
pub(super) struct Sighting {
    pub(super) version: Option<String>,
    pub(super) environment: Option<String>,
    pub(super) at: i64,
}

/// What the errors of one batch say of one issue.
pub(super) struct Seen {
    pub(super) application: String,
    pub(super) id: String,
    /// The rows of the batch that belong to it.
    pub(super) rows: Vec<usize>,
    pub(super) first_at: i64,
    pub(super) first_version: Option<String>,
    pub(super) last_at: i64,
    pub(super) last_version: Option<String>,
    /// The latest time it was seen in each version and environment.
    pub(super) sightings: Vec<Sighting>,
    /// The first time it was seen in each environment.
    pub(super) environments: Vec<(String, i64)>,
}

impl Seen {
    pub(super) fn merge(&mut self, other: Seen) {
        self.rows.extend(other.rows);

        if other.first_at < self.first_at {
            self.first_at = other.first_at;
            self.first_version = other.first_version;
        }

        if other.last_at >= self.last_at {
            self.last_at = other.last_at;
            self.last_version = other.last_version;
        }

        for sighting in other.sightings {
            self.note(sighting);
        }

        for (environment, at) in other.environments {
            self.note_environment(environment, at);
        }
    }

    fn note(&mut self, sighting: Sighting) {
        match self.sightings.iter_mut().find(|known| {
            known.version == sighting.version && known.environment == sighting.environment
        }) {
            Some(known) => known.at = known.at.max(sighting.at),
            None => self.sightings.push(sighting),
        }
    }

    fn note_environment(&mut self, environment: String, at: i64) {
        match self
            .environments
            .iter_mut()
            .find(|(known, _)| *known == environment)
        {
            Some((_, first)) => *first = (*first).min(at),
            None => self.environments.push((environment, at)),
        }
    }
}

/// What the rows of a batch say of the issues they belong to.
pub(super) fn gather(rows: &[RawEvent]) -> Vec<Seen> {
    let mut order: Vec<Seen> = Vec::new();
    let mut at: HashMap<(String, String), usize> = HashMap::new();

    for (index, row) in rows.iter().enumerate() {
        let (Some(application), Some(id)) =
            (text(row, "application.id"), text(row, "error.issue_id"))
        else {
            continue;
        };

        let version = text(row, "version")
            .and_then(version_key)
            .map(str::to_string);
        let environment = text(row, "env")
            .and_then(environment_key)
            .map(str::to_string);
        let when = row.timestamp.0;

        let item = Seen {
            application: application.to_string(),
            id: id.to_string(),
            rows: vec![index],
            first_at: when,
            first_version: version.clone(),
            last_at: when,
            last_version: version.clone(),
            environments: environment
                .iter()
                .map(|environment| (environment.clone(), when))
                .collect(),
            sightings: vec![Sighting {
                version,
                environment,
                at: when,
            }],
        };

        match at.get(&(application.to_string(), id.to_string())) {
            Some(&position) => order[position].merge(item),
            None => {
                at.insert((application.to_string(), id.to_string()), order.len());
                order.push(item);
            }
        }
    }

    order
}

/// `seen` with the entries of one issue made one.
pub(super) fn merged(seen: Vec<Seen>) -> Vec<Seen> {
    let mut order: Vec<Seen> = Vec::new();
    let mut at: HashMap<(String, String), usize> = HashMap::new();

    for item in seen {
        let key = (item.application.clone(), item.id.clone());

        match at.get(&key) {
            Some(&position) => order[position].merge(item),
            None => {
                at.insert(key, order.len());
                order.push(item);
            }
        }
    }

    order
}

/// The issue a first sight opens: what the error that was seen first says it is.
pub(super) fn opening(item: &Seen, rows: &[RawEvent]) -> RumIssue {
    let mut issue = RumIssue::opened(
        item.id.clone(),
        item.application.clone(),
        TimestampMicros(item.first_at),
        item.first_version.clone(),
    );

    issue.last_seen_at = TimestampMicros(item.last_at.max(item.first_at));
    issue.last_version = item.last_version.clone().or(item.first_version.clone());

    if item.id == overflow_id(&item.application) {
        issue.overflow = true;

        return issue;
    }

    let Some(row) = item.rows.first().and_then(|&index| rows.get(index)) else {
        return issue;
    };

    issue.source = cut(
        text(row, "error.source").unwrap_or_default(),
        MAX_SOURCE_LEN,
    )
    .to_string();
    issue.error_type = cut(text(row, "error.type").unwrap_or_default(), MAX_TYPE_LEN).to_string();
    issue.message = cut(
        text(row, "error.message").unwrap_or_default(),
        MAX_MESSAGE_LEN,
    )
    .to_string();
    issue.location = location(&row.fields);

    issue
}

fn text<'a>(row: &'a RawEvent, key: &str) -> Option<&'a str> {
    row.fields
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
}
