// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Email subscribers brought over from another status page as a CSV file:
//! an `email` column, and optionally a `components` column naming the
//! components each address hears about, separated by `;`; a group's name
//! stands for the components in it. A file without a header has the address
//! first and the components second.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use super::{super::StatusPageService, subscription_token, token_hash};
use crate::{
    domain::status_page::{
        ComponentLifecycle, ImportedStatusPageSubscriber, StatusPage, StatusPageLifecycle,
        StatusPageSubscriberChannel, StatusPageSubscriberImport, StatusPageSubscriberImportMode,
        StatusPageSubscriberImportOutcome, SubscriberComponents,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const MAX_IMPORT_BYTES: usize = 2 * 1024 * 1024;
const MAX_IMPORT_ROWS: usize = 10_000;
/// How many rows with a problem a report names; the rest are counted.
const REPORTED_PROBLEMS: usize = 100;

/// A row that was not imported, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusPageImportProblem {
    pub line: u64,
    pub value: String,
    /// Why, as a code the interface words: `missing_email`, `invalid_email`,
    /// `duplicate_email` or `unknown_component`.
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusPageSubscriberImportReport {
    #[serde(flatten)]
    pub outcome: StatusPageSubscriberImportOutcome,
    /// The rows read, header and blank lines aside.
    pub rows: u64,
    pub problems: Vec<StatusPageImportProblem>,
    pub problem_count: u64,
    pub dry_run: bool,
}

impl StatusPageService {
    pub async fn import_subscribers(
        &self,
        org_id: &Id,
        page_id: &Id,
        csv: &str,
        mode: StatusPageSubscriberImportMode,
        dry_run: bool,
    ) -> Result<StatusPageSubscriberImportReport> {
        let page = self.repository.get_page(org_id, page_id).await?;

        if page.lifecycle != StatusPageLifecycle::Active {
            return Err(Error::conflict(
                "subscribers cannot be imported to an archived status page",
            ));
        }

        let sender = self.notification_sender()?;

        if mode == StatusPageSubscriberImportMode::Confirm
            && !sender.delivers(StatusPageSubscriberChannel::Email)
        {
            return Err(Error::unavailable(
                "email delivery is not configured, so no confirmation can be sent",
            ));
        }

        if csv.len() > MAX_IMPORT_BYTES {
            return Err(Error::invalid("an import file can be at most 2 MB"));
        }

        let rows = parse_rows(csv)?;

        if rows.len() > MAX_IMPORT_ROWS {
            return Err(Error::invalid(
                "an import can hold at most 10,000 addresses",
            ));
        }

        let components = self.importable_components(&page).await?;

        let mut problems = Problems::default();
        let mut seen = HashSet::new();
        let mut subscribers = Vec::new();

        for row in &rows {
            let email = row.email.trim().to_ascii_lowercase();

            if email.is_empty() {
                problems.add(row.line, &row.email, "missing_email");
                continue;
            }

            if email.len() > 254
                || sender
                    .validate_target(StatusPageSubscriberChannel::Email, &email)
                    .await
                    .is_err()
            {
                problems.add(row.line, &row.email, "invalid_email");
                continue;
            }

            if !seen.insert(email.clone()) {
                problems.add(row.line, &row.email, "duplicate_email");
                continue;
            }

            let mut chosen = Vec::new();
            let mut unknown = None;

            for name in &row.components {
                let Some(ids) = components.get(&name.to_lowercase()) else {
                    unknown = Some(name.clone());
                    break;
                };

                for id in ids {
                    if !chosen.contains(id) {
                        chosen.push(id.clone());
                    }
                }
            }

            if let Some(name) = unknown {
                problems.add(row.line, &name, "unknown_component");
                continue;
            }

            let token = subscription_token();

            subscribers.push(ImportedStatusPageSubscriber {
                target: email,
                token_hash: token_hash(&token),
                token,
                components: SubscriberComponents::chosen(chosen),
            });
        }

        let outcome = self
            .repository
            .import_subscribers(
                org_id,
                page_id,
                &StatusPageSubscriberImport {
                    mode,
                    subscribers,
                    dry_run,
                    imported_at: TimestampMicros::now(),
                },
            )
            .await?;

        Ok(StatusPageSubscriberImportReport {
            outcome,
            rows: u64::try_from(rows.len()).unwrap_or(u64::MAX),
            problem_count: problems.count,
            problems: problems.reported,
            dry_run,
        })
    }
}

impl StatusPageService {
    /// The components a row can name, by lowercase name. A group's name stands
    /// for the components in it that the page shows.
    async fn importable_components(&self, page: &StatusPage) -> Result<HashMap<String, Vec<Id>>> {
        let shown = self.shown_components(page).await?;
        let mut by_name: HashMap<String, Vec<Id>> = HashMap::new();

        for component in &shown {
            by_name
                .entry(component.name.trim().to_lowercase())
                .or_default()
                .push(component.id.clone());
        }

        let groups = self
            .repository
            .list_components(&page.org_id, &page.id)
            .await?
            .into_iter()
            .filter(|component| {
                component.is_group() && component.lifecycle == ComponentLifecycle::Active
            });

        for group in groups {
            let members: Vec<Id> = shown
                .iter()
                .filter(|component| component.is_in(&group.id))
                .map(|component| component.id.clone())
                .collect();

            if !members.is_empty() {
                by_name
                    .entry(group.name.trim().to_lowercase())
                    .or_default()
                    .extend(members);
            }
        }

        Ok(by_name)
    }
}

#[derive(Default)]
struct Problems {
    reported: Vec<StatusPageImportProblem>,
    count: u64,
}

impl Problems {
    fn add(&mut self, line: u64, value: &str, reason: &str) {
        self.count += 1;

        if self.reported.len() < REPORTED_PROBLEMS {
            self.reported.push(StatusPageImportProblem {
                line,
                value: value.chars().take(200).collect(),
                reason: reason.to_string(),
            });
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CsvRow {
    line: u64,
    email: String,
    components: Vec<String>,
}

fn parse_rows(csv: &str) -> Result<Vec<CsvRow>> {
    let text = csv.trim_start_matches('\u{feff}');

    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(text.as_bytes());

    // Without a header the address comes first and the components second.
    let mut columns = (0, Some(1));
    let mut first = true;
    let mut rows = Vec::new();

    for record in reader.records() {
        let record = record
            .map_err(|error| Error::invalid(format!("the file is not valid CSV: {error}")))?;

        if record.iter().all(str::is_empty) {
            continue;
        }

        if std::mem::take(&mut first)
            && let Some(header) = header_columns(&record)
        {
            columns = header;
            continue;
        }

        rows.push(CsvRow {
            line: record
                .position()
                .map_or(0, |position| record_line(text, position)),
            email: record.get(columns.0).unwrap_or_default().to_string(),
            components: columns
                .1
                .and_then(|index| record.get(index))
                .map(component_names)
                .unwrap_or_default(),
        });
    }

    Ok(rows)
}

/// The line a record starts on. The reader reports where it began looking for
/// the record, before the blank lines it skipped on the way.
fn record_line(text: &str, position: &csv::Position) -> u64 {
    let start = usize::try_from(position.byte())
        .unwrap_or(text.len())
        .min(text.len());

    let skipped = text.as_bytes()[start..]
        .iter()
        .take_while(|byte| matches!(byte, b'\r' | b'\n'))
        .filter(|byte| **byte == b'\n')
        .count();

    position.line() + u64::try_from(skipped).unwrap_or_default()
}

/// The columns of the address and of the components, when the first row is a
/// header.
fn header_columns(record: &csv::StringRecord) -> Option<(usize, Option<usize>)> {
    let names: Vec<String> = record
        .iter()
        .map(|cell| cell.trim().to_lowercase())
        .collect();

    let email = names.iter().position(|name| {
        matches!(
            name.as_str(),
            "email" | "e-mail" | "email address" | "mail" | "邮箱" | "邮件地址"
        )
    })?;

    let components = names
        .iter()
        .position(|name| matches!(name.as_str(), "components" | "component" | "组件"));

    Some((email, components))
}

fn component_names(cell: &str) -> Vec<String> {
    cell.split([';', '|'])
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_names_the_columns_in_any_order() {
        let rows = parse_rows(
            "\u{feff}Components,Email\nAPI; Dashboard,ops@example.com\n\n,dev@example.com\n",
        )
        .unwrap();

        assert_eq!(
            rows,
            [
                CsvRow {
                    line: 2,
                    email: "ops@example.com".into(),
                    components: vec!["API".into(), "Dashboard".into()],
                },
                CsvRow {
                    line: 4,
                    email: "dev@example.com".into(),
                    components: Vec::new(),
                },
            ]
        );
    }

    #[test]
    fn without_a_header_the_address_comes_first() {
        let rows = parse_rows("ops@example.com\n\"dev@example.com\",\"API|EU region\"\n").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].line, 1);
        assert_eq!(rows[1].components, ["API", "EU region"]);
    }

    #[test]
    fn a_report_names_the_first_problems_and_counts_the_rest() {
        let mut problems = Problems::default();

        for line in 0..150 {
            problems.add(line, "x", "invalid_email");
        }

        assert_eq!(problems.count, 150);
        assert_eq!(problems.reported.len(), REPORTED_PROBLEMS);
    }
}
