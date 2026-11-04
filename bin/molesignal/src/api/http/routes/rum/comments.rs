// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Comments on the replay of a session, each at a moment of it.
//!
//! - `GET /rum/sessions/{id}/comments` lists them, earliest moment first;
//! - `POST /rum/sessions/{id}/comments` adds one;
//! - `PATCH` and `DELETE /rum/sessions/{id}/comments/{comment_id}` change or remove one.
//!
//! Whoever may watch the replay reads them. Writing takes `rum.replay.comments.write`, and that is
//! enough for one's own; another's takes `rum.replay.comments.manage`.

use axum::{
    Extension, Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{
        iam::permission,
        rum::{RumReplayComment, validate_application_id, validate_comment_body},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CommentView {
    id: String,
    application: String,
    session_id: String,
    at_time: i64,
    body: String,
    author_id: Option<String>,
    author_name: Option<String>,
    created_at: i64,
    updated_at: i64,
    /// Whether the reader may change or remove it.
    editable: bool,
}

fn view(ctx: &IamContext, comment: RumReplayComment) -> CommentView {
    let editable = may_change(ctx, &comment);

    CommentView {
        id: comment.id,
        application: comment.application_id,
        session_id: comment.session_id,
        at_time: comment.at_time,
        body: comment.body,
        author_id: comment.author_id.map(|id| id.0),
        author_name: comment.author_name,
        created_at: comment.created_at.0,
        updated_at: comment.updated_at.0,
        editable,
    }
}

/// One's own comment takes writing comments; another's takes managing them.
fn may_change(ctx: &IamContext, comment: &RumReplayComment) -> bool {
    ctx.has_permission("rum.replay.comments.manage")
        || (ctx.has_permission("rum.replay.comments.write")
            && comment.author_id.as_ref() == Some(&ctx.user_id))
}

#[derive(Debug, Serialize)]
pub(super) struct CommentList {
    comments: Vec<CommentView>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CreateComment {
    application: String,
    /// The moment of the session it is about, in microseconds since the epoch.
    at_time: i64,
    body: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateComment {
    body: String,
}

fn session_of(id: &str) -> Result<&str> {
    let id = id.trim();

    if id.is_empty() || id.len() > 128 {
        return Err(Error::invalid(
            "a session is named by an id of 1 to 128 characters",
        ));
    }

    Ok(id)
}

#[permission(any("rum.replay.read", "sys.telemetry.read"))]
pub(super) async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(session_id): Path<String>,
) -> Result<Json<CommentList>> {
    let comments = state
        .telemetry
        .rum_comments
        .list(&ctx.org_id, session_of(&session_id)?)
        .await?;

    Ok(Json(CommentList {
        comments: comments
            .into_iter()
            .map(|comment| view(&ctx, comment))
            .collect(),
    }))
}

#[permission("rum.replay.comments.write")]
pub(super) async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(session_id): Path<String>,
    Json(request): Json<CreateComment>,
) -> Result<Json<CommentView>> {
    if ctx.is_system_scope() {
        return Err(Error::not_found("RUM replay comments"));
    }

    if request.at_time <= 0 {
        return Err(Error::invalid("atTime must be a moment after the epoch"));
    }

    let application = validate_application_id(&request.application)?.to_string();

    // A comment is of an application that exists.
    state
        .telemetry
        .rum_applications
        .get(&ctx.org_id, &application)
        .await?;

    let now = TimestampMicros::now();

    let comment = state
        .telemetry
        .rum_comments
        .create(RumReplayComment {
            org_id: ctx.org_id.clone(),
            id: Id::new().0,
            application_id: application,
            session_id: session_of(&session_id)?.to_string(),
            at_time: request.at_time,
            body: validate_comment_body(&request.body)?.to_string(),
            author_id: Some(ctx.user_id.clone()),
            author_name: None,
            created_at: now,
            updated_at: now,
        })
        .await?;

    Ok(Json(view(&ctx, comment)))
}

#[permission("rum.replay.comments.write")]
pub(super) async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path((session_id, comment_id)): Path<(String, String)>,
    Json(request): Json<UpdateComment>,
) -> Result<Json<CommentView>> {
    let session_id = session_of(&session_id)?;
    let body = validate_comment_body(&request.body)?;

    let held = state
        .telemetry
        .rum_comments
        .get(&ctx.org_id, session_id, &comment_id)
        .await?;

    if !may_change(&ctx, &held) {
        return Err(Error::forbidden("this comment is someone else's"));
    }

    let comment = state
        .telemetry
        .rum_comments
        .update_body(
            &ctx.org_id,
            session_id,
            &comment_id,
            body,
            TimestampMicros::now(),
        )
        .await?;

    Ok(Json(view(&ctx, comment)))
}

#[permission(any("rum.replay.comments.write", "rum.replay.comments.manage"))]
pub(super) async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path((session_id, comment_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    let session_id = session_of(&session_id)?;

    let held = state
        .telemetry
        .rum_comments
        .get(&ctx.org_id, session_id, &comment_id)
        .await?;

    if !may_change(&ctx, &held) {
        return Err(Error::forbidden("this comment is someone else's"));
    }

    state
        .telemetry
        .rum_comments
        .delete(&ctx.org_id, session_id, &comment_id)
        .await?;

    Ok(Json(serde_json::json!({ "deleted": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_is_named_by_an_id() {
        assert_eq!(session_of(" s-1 ").unwrap(), "s-1");
        assert!(session_of("  ").is_err());
        assert!(session_of(&"x".repeat(129)).is_err());
    }

    #[test]
    fn a_request_names_what_it_changes_and_no_more() {
        assert!(serde_json::from_str::<UpdateComment>(r#"{"body":"x"}"#).is_ok());
        assert!(serde_json::from_str::<UpdateComment>(r#"{"body":"x","at_time":1}"#).is_err());
    }
}
