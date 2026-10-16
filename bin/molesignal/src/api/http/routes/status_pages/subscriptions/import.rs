// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Email subscribers brought over from another status page as CSV.

use axum::{
    Extension, Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    routing::post,
};
use serde::Deserialize;

use crate::{
    api::{AppState, http::routes::activity_audit},
    app::{iam::IamContext, status_page::StatusPageSubscriberImportReport},
    domain::{iam::permission, status_page::StatusPageSubscriberImportMode},
    shared::{Result, ids::Id},
};

/// A 2 MB file, written into JSON.
const MAX_IMPORT_REQUEST_BYTES: usize = 3 * 1024 * 1024;

pub(super) fn routes() -> Router<AppState> {
    Router::new().route(
        "/status-pages/{page_id}/subscribers/import",
        post(import).layer(DefaultBodyLimit::max(MAX_IMPORT_REQUEST_BYTES)),
    )
}

#[derive(Debug, Deserialize)]
struct ImportRequest {
    csv: String,
    mode: StatusPageSubscriberImportMode,
    /// Counts what the import would do without changing anything.
    #[serde(default)]
    dry_run: bool,
}

#[permission("status_pages.manage")]
async fn import(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
    Json(request): Json<ImportRequest>,
) -> Result<Json<StatusPageSubscriberImportReport>> {
    let page_id = Id(page_id);

    let report = state
        .status_pages
        .import_subscribers(
            &context.org_id,
            &page_id,
            &request.csv,
            request.mode,
            request.dry_run,
        )
        .await?;

    if !request.dry_run {
        activity_audit::record(
            &state,
            &context,
            "status_page.subscriber.import",
            "status_page",
            page_id.as_str(),
            serde_json::json!({
                "mode": request.mode,
                "imported": report.outcome.imported,
                "already_subscribed": report.outcome.already_subscribed,
                "skipped_unsubscribed": report.outcome.skipped_unsubscribed,
                "confirmations_queued": report.outcome.confirmations_queued,
                "problem_count": report.problem_count,
            }),
        )
        .await;
    }

    Ok(Json(report))
}
