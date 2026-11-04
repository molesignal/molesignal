// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Playlists of RUM sessions: the sessions a team keeps to go through one after the other.
//!
//! - `GET /rum/playlists` lists them, of one application with `application`;
//! - `POST /rum/playlists` creates one, with the sessions it starts with;
//! - `GET /rum/playlists/{id}` reads one with its sessions, `PATCH` renames it, `DELETE` removes it;
//! - `POST /rum/playlists/{id}/sessions` adds sessions to it, each with the moment worth opening
//!   it at, and `DELETE /rum/playlists/{id}/sessions/{session_id}` takes one out;
//! - `GET /rum/sessions/{id}/playlists` lists the playlists a session is in.
//!
//! A playlist is of one application and is seen by everyone who may read RUM. It holds the ids of
//! sessions, not their data: the session is read where it is, so a session that is gone from the
//! streams is a row the page cannot open. Changing a playlist takes `rum.playlists.manage`.

use std::collections::BTreeMap;

use axum::{
    Extension, Json,
    extract::{Path, Query, State},
};
use serde::{Deserialize, Serialize};

use crate::{
    api::{AppState, http::middleware::Permission},
    app::iam::IamContext,
    domain::{
        iam::permission,
        rum::{
            MAX_PLAYLIST_SESSIONS, RumPlaylist, RumPlaylistEntry, RumPlaylistPatch,
            validate_playlist_description, validate_playlist_name, validate_playlist_session,
        },
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PlaylistView {
    id: String,
    application: String,
    name: String,
    description: String,
    created_by: Option<String>,
    created_at: i64,
    updated_at: i64,
    session_count: u32,
}

impl From<RumPlaylist> for PlaylistView {
    fn from(playlist: RumPlaylist) -> Self {
        Self {
            id: playlist.id,
            application: playlist.application_id,
            name: playlist.name,
            description: playlist.description,
            created_by: playlist.created_by.map(|id| id.0),
            created_at: playlist.created_at.0,
            updated_at: playlist.updated_at.0,
            session_count: playlist.session_count,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct EntryView {
    session_id: String,
    at_time: Option<i64>,
    added_by: Option<String>,
    added_at: i64,
}

impl From<RumPlaylistEntry> for EntryView {
    fn from(entry: RumPlaylistEntry) -> Self {
        Self {
            session_id: entry.session_id,
            at_time: entry.at_time,
            added_by: entry.added_by.map(|id| id.0),
            added_at: entry.added_at.0,
        }
    }
}

/// A playlist with the sessions in it, in the order they were added.
#[derive(Debug, Serialize)]
pub(super) struct PlaylistDetail {
    #[serde(flatten)]
    playlist: PlaylistView,
    sessions: Vec<EntryView>,
}

/// A session a request puts in a playlist.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SessionInput {
    session_id: String,
    /// The moment to open it at, in microseconds since the epoch; none opens it at its beginning.
    #[serde(default)]
    at_time: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CreatePlaylist {
    application: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    sessions: Vec<SessionInput>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AddSessions {
    sessions: Vec<SessionInput>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ListQuery {
    application: Option<String>,
}

/// The sessions of a request as playlist entries: each one once (the last moment given for it
/// counts), by the user who put them there, and no more than a playlist holds.
fn entries_of(
    sessions: Vec<SessionInput>,
    added_by: &Id,
    now: TimestampMicros,
) -> Result<Vec<RumPlaylistEntry>> {
    let mut found = BTreeMap::new();

    for session in sessions {
        let session_id = validate_playlist_session(&session.session_id)?.to_string();

        if session.at_time.is_some_and(|at| at <= 0) {
            return Err(Error::invalid("atTime must be a moment after the epoch"));
        }

        found.insert(session_id, session.at_time);
    }

    if found.len() > MAX_PLAYLIST_SESSIONS {
        return Err(Error::invalid(format!(
            "a playlist holds at most {MAX_PLAYLIST_SESSIONS} sessions"
        )));
    }

    Ok(found
        .into_iter()
        .map(|(session_id, at_micros)| RumPlaylistEntry {
            session_id,
            at_time: at_micros,
            added_by: Some(added_by.clone()),
            added_at: now,
        })
        .collect())
}

async fn detail(state: &AppState, org_id: &Id, id: &str) -> Result<Json<PlaylistDetail>> {
    let playlist = state.telemetry.rum_playlists.get(org_id, id).await?;
    let sessions = state.telemetry.rum_playlists.entries(org_id, id).await?;

    Ok(Json(PlaylistDetail {
        playlist: playlist.into(),
        sessions: sessions.into_iter().map(EntryView::from).collect(),
    }))
}

#[permission(any("rum.playlists.read", "sys.telemetry.read"))]
pub(super) async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<PlaylistView>>> {
    let application = query
        .application
        .as_deref()
        .map(str::trim)
        .filter(|application| !application.is_empty());

    let playlists = state
        .telemetry
        .rum_playlists
        .list(&ctx.org_id, application)
        .await?
        .into_iter()
        .map(PlaylistView::from)
        .collect();

    Ok(Json(playlists))
}

/// `GET /rum/sessions/{id}/playlists`: the playlists a session is in.
#[permission(any("rum.playlists.read", "sys.telemetry.read"))]
pub(super) async fn of_session(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(session_id): Path<String>,
) -> Result<Json<Vec<PlaylistView>>> {
    let playlists = state
        .telemetry
        .rum_playlists
        .of_session(&ctx.org_id, session_id.trim())
        .await?
        .into_iter()
        .map(PlaylistView::from)
        .collect();

    Ok(Json(playlists))
}

#[permission(any("rum.playlists.read", "sys.telemetry.read"))]
pub(super) async fn get(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<PlaylistDetail>> {
    detail(&state, &ctx.org_id, &id).await
}

pub(super) async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(body): Json<CreatePlaylist>,
) -> Result<Json<PlaylistDetail>> {
    require_manage(&ctx)?;
    validate_playlist_name(&body.name)?;
    validate_playlist_description(&body.description)?;

    let now = TimestampMicros::now();
    let entries = entries_of(body.sessions, &ctx.user_id, now)?;

    // A playlist is of an application that exists, as an event is.
    let application = state
        .telemetry
        .rum_applications
        .get(&ctx.org_id, body.application.trim())
        .await?;

    let playlist = state
        .telemetry
        .rum_playlists
        .create(
            RumPlaylist {
                org_id: ctx.org_id.clone(),
                id: Id::new().0,
                application_id: application.id,
                name: body.name.trim().to_string(),
                description: body.description,
                created_by: Some(ctx.user_id.clone()),
                created_at: now,
                updated_at: now,
                session_count: 0,
            },
            entries,
        )
        .await?;

    detail(&state, &ctx.org_id, &playlist.id).await
}

pub(super) async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(patch): Json<RumPlaylistPatch>,
) -> Result<Json<PlaylistDetail>> {
    require_manage(&ctx)?;
    patch.validate()?;

    state
        .telemetry
        .rum_playlists
        .update(&ctx.org_id, &id, patch)
        .await?;

    detail(&state, &ctx.org_id, &id).await
}

pub(super) async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    require_manage(&ctx)?;

    state
        .telemetry
        .rum_playlists
        .delete(&ctx.org_id, &id)
        .await?;

    Ok(Json(serde_json::json!({ "deleted": true })))
}

pub(super) async fn add_sessions(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(body): Json<AddSessions>,
) -> Result<Json<PlaylistDetail>> {
    require_manage(&ctx)?;

    let entries = entries_of(body.sessions, &ctx.user_id, TimestampMicros::now())?;

    state
        .telemetry
        .rum_playlists
        .add(&ctx.org_id, &id, entries)
        .await?;

    detail(&state, &ctx.org_id, &id).await
}

pub(super) async fn remove_session(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path((id, session_id)): Path<(String, String)>,
) -> Result<Json<PlaylistDetail>> {
    require_manage(&ctx)?;

    state
        .telemetry
        .rum_playlists
        .remove(&ctx.org_id, &id, &session_id)
        .await?;

    detail(&state, &ctx.org_id, &id).await
}

/// Changing what a team keeps is managing RUM, as setting the state of an issue is.
fn require_manage(ctx: &IamContext) -> Result<()> {
    if ctx.is_system_scope() {
        return Err(Error::not_found("RUM playlists"));
    }

    Permission::require_key(ctx, "rum.playlists.manage")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(session_id: &str, at_micros: Option<i64>) -> SessionInput {
        SessionInput {
            session_id: session_id.into(),
            at_time: at_micros,
        }
    }

    #[test]
    fn a_session_is_put_in_once_with_the_last_moment_given_for_it() {
        let entries = entries_of(
            vec![
                input(" s-2 ", None),
                input("s-1", Some(10)),
                input("s-2", Some(30)),
                input("s-1", Some(20)),
            ],
            &Id::from_string("user-1"),
            TimestampMicros(5),
        )
        .unwrap();

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].session_id, "s-1");
        assert_eq!(entries[0].at_time, Some(20));
        assert_eq!(entries[1].session_id, "s-2");
        assert_eq!(entries[1].at_time, Some(30));
        assert!(
            entries
                .iter()
                .all(|entry| entry.added_at == TimestampMicros(5))
        );
        assert!(
            entries
                .iter()
                .all(|entry| entry.added_by == Some(Id::from_string("user-1")))
        );
    }

    #[test]
    fn what_cannot_be_a_session_or_a_moment_is_refused() {
        let user = Id::from_string("user-1");
        let now = TimestampMicros(1);

        assert!(entries_of(vec![input("  ", None)], &user, now).is_err());
        assert!(entries_of(vec![input(&"s".repeat(129), None)], &user, now).is_err());
        assert!(entries_of(vec![input("s-1", Some(0))], &user, now).is_err());
        assert!(entries_of(vec![input("s-1", Some(-4))], &user, now).is_err());
        assert!(entries_of(Vec::new(), &user, now).unwrap().is_empty());
    }

    #[test]
    fn a_request_cannot_hold_more_sessions_than_a_playlist_does() {
        let sessions = (0..=MAX_PLAYLIST_SESSIONS)
            .map(|n| input(&format!("s-{n}"), None))
            .collect();

        assert!(entries_of(sessions, &Id::from_string("u"), TimestampMicros(1)).is_err());

        let enough = (0..MAX_PLAYLIST_SESSIONS)
            .map(|n| input(&format!("s-{n}"), None))
            .collect();

        assert_eq!(
            entries_of(enough, &Id::from_string("u"), TimestampMicros(1))
                .unwrap()
                .len(),
            MAX_PLAYLIST_SESSIONS
        );
    }

    #[test]
    fn a_playlist_is_shown_with_its_times_in_microseconds_and_the_sessions_it_holds() {
        let view = PlaylistDetail {
            playlist: RumPlaylist {
                org_id: Id::from_string("org"),
                id: "pl-1".into(),
                application_id: "shop-web".into(),
                name: "Checkout failures".into(),
                description: String::new(),
                created_by: Some(Id::from_string("user-1")),
                created_at: TimestampMicros(10),
                updated_at: TimestampMicros(20),
                session_count: 1,
            }
            .into(),
            sessions: vec![
                RumPlaylistEntry {
                    session_id: "s-1".into(),
                    at_time: Some(15),
                    added_by: None,
                    added_at: TimestampMicros(12),
                }
                .into(),
            ],
        };

        let value = serde_json::to_value(view).unwrap();

        assert_eq!(value["id"], "pl-1");
        assert_eq!(value["application"], "shop-web");
        assert_eq!(value["createdAt"], 10);
        assert_eq!(value["sessionCount"], 1);
        assert_eq!(value["sessions"][0]["sessionId"], "s-1");
        assert_eq!(value["sessions"][0]["atTime"], 15);
        assert!(value.get("orgId").is_none());
    }

    #[test]
    fn a_new_playlist_needs_an_application_and_a_name_and_nothing_else_is_taken() {
        let body: CreatePlaylist = serde_json::from_value(serde_json::json!({
            "application": "shop-web",
            "name": "Checkout failures",
            "sessions": [{ "sessionId": "s-1", "atTime": 5 }]
        }))
        .unwrap();

        assert_eq!(body.application, "shop-web");
        assert_eq!(body.description, "");
        assert_eq!(body.sessions.len(), 1);

        assert!(
            serde_json::from_value::<CreatePlaylist>(serde_json::json!({ "name": "x" })).is_err()
        );
        assert!(
            serde_json::from_value::<CreatePlaylist>(
                serde_json::json!({ "application": "a", "name": "x", "orgId": "o" })
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<SessionInput>(
                serde_json::json!({ "sessionId": "s", "note": "x" })
            )
            .is_err()
        );
    }
}
