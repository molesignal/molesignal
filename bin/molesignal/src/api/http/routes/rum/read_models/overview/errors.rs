// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::{HashMap, HashSet};

use super::{super::ReadModelContext, model::FrequentError};
use crate::{
    api::AppState,
    app::iam::IamContext,
    infra::rum::read_model::{RumReadModelReader, RumScope},
    shared::Result,
};

const RESULT_LIMIT: usize = 5;
const COLUMNS: &[&str] = &[
    "_timestamp",
    "fingerprint",
    "message",
    "user_id",
    "version",
    "application",
    "environment",
];

#[derive(Default)]
struct ErrorGroup {
    message: String,
    users: HashSet<String>,
    version: Option<String>,
    count: i64,
}

pub(super) async fn load(
    state: &AppState,
    iam: &IamContext,
    context: &ReadModelContext,
) -> Result<Vec<FrequentError>> {
    let reader = RumReadModelReader::new(
        state.storage.parquet_file_meta.clone(),
        state.storage.object_store.clone(),
    );
    let scope = RumScope {
        application: context.application.as_deref(),
        environment: context.environment.as_deref(),
        version: context.version.as_deref(),
        ..RumScope::default()
    };
    let mut groups = HashMap::<String, ErrorGroup>::new();
    let stats = reader
        .visit_errors(&iam.org_id, context.time_range(), COLUMNS, |record| {
            if !record.matches_scope(scope) {
                return;
            }
            let group = groups.entry(record.fingerprint).or_default();
            group.count += 1;
            if let Some(message) = record.message
                && (group.message.is_empty() || message < group.message)
            {
                group.message = message;
            }
            if let Some(user) = record.user_id {
                group.users.insert(user);
            }
            if let Some(version) = record.version
                && group
                    .version
                    .as_ref()
                    .is_none_or(|current| version < *current)
            {
                group.version = Some(version);
            }
        })
        .await?;
    let mut groups = groups.into_iter().collect::<Vec<_>>();
    groups.sort_by(|(left_key, left), (right_key, right)| {
        right
            .users
            .len()
            .cmp(&left.users.len())
            .then_with(|| right.count.cmp(&left.count))
            .then_with(|| left_key.cmp(right_key))
    });
    let errors = groups
        .into_iter()
        .take(RESULT_LIMIT)
        .map(|(fingerprint, group)| FrequentError {
            fingerprint,
            message: group.message,
            users: i64::try_from(group.users.len()).unwrap_or(i64::MAX),
            version: group.version,
        })
        .collect();
    tracing::debug!(
        scanned_files = stats.files,
        scanned_rows = stats.rows,
        "RUM overview error read model completed"
    );
    Ok(errors)
}
