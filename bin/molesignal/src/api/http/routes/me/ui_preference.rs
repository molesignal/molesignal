// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! How the signed-in user prefers a page to look: read, written and cleared by
//! purpose and key.
//!
//! Every purpose is known here with the shape its preference takes, so the
//! table holds what a page of this product wrote and nothing else. A preference
//! is the user's own, in the organization they are signed in to: no permission
//! is asked for it, and nobody else can read it.

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    api::AppState,
    app::iam::IamContext,
    infra::persistence::repositories::user_ui_preference::UiPreferenceKey,
    shared::{Error, Result},
};

/// The longest a key is: a stream's name, a dashboard's id.
const MAX_KEY_CHARS: usize = 255;
/// The most a preference takes when written out. It is an arrangement, not a document.
const MAX_VALUE_BYTES: usize = 16 * 1024;
/// The most columns an arrangement names.
const MAX_LOG_COLUMNS: usize = 64;
/// The most answers one keeps pinned in a chat.
const MAX_PINNED_MESSAGES: usize = 100;
/// The longest a message's id is.
const MAX_MESSAGE_ID_CHARS: usize = 64;
/// The most fields of a trace's root shown beside the traces: what the trace list reads at once.
const MAX_TRACE_COLUMNS: usize = 12;

/// The most columns an APM table shows.
const MAX_APM_COLUMNS: usize = 16;
/// The most services one keeps among their APM favorites.
const MAX_APM_FAVORITES: usize = 200;
/// The most notifications remembered as read; the web forgets what left the bell first.
const MAX_READ_NOTIFICATIONS: usize = 200;
/// The longest a notification's id is: a kind, an id, a time.
const MAX_NOTIFICATION_ID_CHARS: usize = 128;
/// The most views of the RUM explorer one keeps, the longest a name of one is, and the longest its
/// address: what is on the page, not a document.
const MAX_RUM_SAVED_VIEWS: usize = 30;
const MAX_RUM_VIEW_NAME_CHARS: usize = 64;
const MAX_RUM_VIEW_SEARCH_CHARS: usize = 2_048;
/// The widths a trace view keeps, in CSS pixels: wide enough to use, no wider than a screen.
const TRACE_LAYOUT_WIDTHS: std::ops::RangeInclusive<u32> = 160..=4_000;

pub(super) fn routes() -> Router<AppState> {
    Router::new().route(
        "/me/ui-preference/{scope}/{key}",
        get(read).put(write).delete(clear),
    )
}

/// What an interface preference is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    /// The columns a log stream is listed in, in their order. Keyed by the stream's name.
    LogColumns,
    /// The answers of a Mole Agent chat kept at the top of it, the last pinned first. Keyed
    /// by the chat's id.
    AgentMessagePins,
    /// The fields of a trace's root Span shown as columns of the trace list, in their order.
    /// Keyed by the trace stream's name.
    TraceColumns,
    /// How wide the parts of a trace's view are drawn. One key, `view`.
    TraceLayout,
    /// The columns of an APM table, in their order. Keyed by the table (`catalog`, `endpoints`).
    ApmColumns,
    /// How wide APM's drawers are drawn. One key, `view`.
    ApmLayout,
    /// How wide the drawer a log opens in is drawn. One key, `view`.
    LogLayout,
    /// The services one keeps at the top of APM's catalog. One key, `services`.
    ApmFavorites,
    /// The notifications one has read, so the bell stops ringing for them. One key, `read`.
    NotificationsRead,
    /// The views of the RUM explorer one keeps to come back to. One key, `explorer`.
    RumSavedViews,
}

impl Scope {
    fn parse(raw: &str) -> Result<Self> {
        match raw {
            "logs.columns" => Ok(Self::LogColumns),
            "agent.message_pins" => Ok(Self::AgentMessagePins),
            "traces.columns" => Ok(Self::TraceColumns),
            "traces.layout" => Ok(Self::TraceLayout),
            "apm.columns" => Ok(Self::ApmColumns),
            "apm.layout" => Ok(Self::ApmLayout),
            "logs.layout" => Ok(Self::LogLayout),
            "apm.favorites" => Ok(Self::ApmFavorites),
            "notifications.read" => Ok(Self::NotificationsRead),
            "rum.saved_views" => Ok(Self::RumSavedViews),
            _ => Err(Error::invalid(format!(
                "unknown interface preference: {raw}"
            ))),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::LogColumns => "logs.columns",
            Self::AgentMessagePins => "agent.message_pins",
            Self::TraceColumns => "traces.columns",
            Self::TraceLayout => "traces.layout",
            Self::ApmColumns => "apm.columns",
            Self::ApmLayout => "apm.layout",
            Self::LogLayout => "logs.layout",
            Self::ApmFavorites => "apm.favorites",
            Self::NotificationsRead => "notifications.read",
            Self::RumSavedViews => "rum.saved_views",
        }
    }

    /// Whether `value` is a preference of this purpose.
    fn validate(self, value: &Value) -> Result<()> {
        match self {
            Self::LogColumns => validate_log_columns(value),
            Self::AgentMessagePins => validate_message_pins(value),
            Self::TraceColumns => validate_trace_columns(value),
            Self::TraceLayout => validate_trace_layout(value),
            Self::ApmColumns => validate_apm_columns(value),
            Self::ApmLayout => validate_drawer_layout(value, "APM layout"),
            Self::LogLayout => validate_drawer_layout(value, "log layout"),
            Self::ApmFavorites => validate_apm_favorites(value),
            Self::NotificationsRead => validate_notifications_read(value),
            Self::RumSavedViews => validate_rum_saved_views(value),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LogColumns {
    fields: Vec<String>,
}

fn validate_log_columns(value: &Value) -> Result<()> {
    let columns: LogColumns = serde_json::from_value(value.clone())
        .map_err(|error| Error::invalid(format!("log columns: {error}")))?;

    if columns.fields.len() > MAX_LOG_COLUMNS {
        return Err(Error::invalid(format!(
            "log columns: at most {MAX_LOG_COLUMNS} fields"
        )));
    }

    for (index, field) in columns.fields.iter().enumerate() {
        if field.is_empty() || field.chars().count() > MAX_KEY_CHARS {
            return Err(Error::invalid(
                "log columns: a field name is 1 to 255 characters",
            ));
        }

        if columns.fields[..index].contains(field) {
            return Err(Error::invalid(format!(
                "log columns: {field} is listed twice"
            )));
        }
    }

    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RumSavedViews {
    views: Vec<RumSavedView>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RumSavedView {
    id: String,
    name: String,
    /// The address of the view: what the explorer reads to show it.
    search: String,
}

fn validate_rum_saved_views(value: &Value) -> Result<()> {
    let kept: RumSavedViews = serde_json::from_value(value.clone())
        .map_err(|error| Error::invalid(format!("RUM saved views: {error}")))?;

    if kept.views.len() > MAX_RUM_SAVED_VIEWS {
        return Err(Error::invalid(format!(
            "RUM saved views: at most {MAX_RUM_SAVED_VIEWS} views"
        )));
    }

    for (index, view) in kept.views.iter().enumerate() {
        if view.id.is_empty() || view.id.chars().count() > MAX_KEY_CHARS {
            return Err(Error::invalid(
                "RUM saved views: an id is 1 to 255 characters",
            ));
        }

        if view.name.trim().is_empty() || view.name.chars().count() > MAX_RUM_VIEW_NAME_CHARS {
            return Err(Error::invalid(format!(
                "RUM saved views: a name is 1 to {MAX_RUM_VIEW_NAME_CHARS} characters"
            )));
        }

        if view.search.chars().count() > MAX_RUM_VIEW_SEARCH_CHARS {
            return Err(Error::invalid(format!(
                "RUM saved views: an address is at most {MAX_RUM_VIEW_SEARCH_CHARS} characters"
            )));
        }

        if kept.views[..index].iter().any(|other| other.id == view.id) {
            return Err(Error::invalid(format!(
                "RUM saved views: {} is listed twice",
                view.id
            )));
        }
    }

    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MessagePins {
    messages: Vec<String>,
}

fn validate_message_pins(value: &Value) -> Result<()> {
    let pins: MessagePins = serde_json::from_value(value.clone())
        .map_err(|error| Error::invalid(format!("pinned answers: {error}")))?;

    if pins.messages.len() > MAX_PINNED_MESSAGES {
        return Err(Error::invalid(format!(
            "pinned answers: at most {MAX_PINNED_MESSAGES}"
        )));
    }

    for (index, message) in pins.messages.iter().enumerate() {
        if message.is_empty() || message.chars().count() > MAX_MESSAGE_ID_CHARS {
            return Err(Error::invalid(
                "pinned answers: a message id is 1 to 64 characters",
            ));
        }

        if pins.messages[..index].contains(message) {
            return Err(Error::invalid(format!(
                "pinned answers: {message} is pinned twice"
            )));
        }
    }

    Ok(())
}

fn validate_trace_columns(value: &Value) -> Result<()> {
    let columns: LogColumns = serde_json::from_value(value.clone())
        .map_err(|error| Error::invalid(format!("trace columns: {error}")))?;

    if columns.fields.len() > MAX_TRACE_COLUMNS {
        return Err(Error::invalid(format!(
            "trace columns: at most {MAX_TRACE_COLUMNS} fields"
        )));
    }

    for (index, field) in columns.fields.iter().enumerate() {
        if field.is_empty() || field.chars().count() > MAX_KEY_CHARS {
            return Err(Error::invalid(
                "trace columns: a field name is 1 to 255 characters",
            ));
        }

        if columns.fields[..index].contains(field) {
            return Err(Error::invalid(format!(
                "trace columns: {field} is listed twice"
            )));
        }
    }

    Ok(())
}

/// The widths of a trace's view: the drawer it opens in over the list, the
/// detail of a span beside the waterfall, and the column of span names.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TraceLayout {
    drawer: Option<u32>,
    panel: Option<u32>,
    names: Option<u32>,
}

fn validate_trace_layout(value: &Value) -> Result<()> {
    let layout: TraceLayout = serde_json::from_value(value.clone())
        .map_err(|error| Error::invalid(format!("trace layout: {error}")))?;

    let widths = [layout.drawer, layout.panel, layout.names];

    if widths
        .into_iter()
        .flatten()
        .any(|width| !TRACE_LAYOUT_WIDTHS.contains(&width))
    {
        return Err(Error::invalid(
            "trace layout: a width is 160 to 4000 pixels",
        ));
    }

    Ok(())
}

fn validate_apm_columns(value: &Value) -> Result<()> {
    let columns: LogColumns = serde_json::from_value(value.clone())
        .map_err(|error| Error::invalid(format!("APM columns: {error}")))?;

    if columns.fields.len() > MAX_APM_COLUMNS {
        return Err(Error::invalid(format!(
            "APM columns: at most {MAX_APM_COLUMNS} columns"
        )));
    }

    for (index, field) in columns.fields.iter().enumerate() {
        if field.is_empty() || field.chars().count() > 64 {
            return Err(Error::invalid(
                "APM columns: a column is 1 to 64 characters",
            ));
        }

        if columns.fields[..index].contains(field) {
            return Err(Error::invalid(format!(
                "APM columns: {field} is listed twice"
            )));
        }
    }

    Ok(())
}

/// A service among one's favorites, as APM names it apart from its environment.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct FavoriteService {
    namespace: String,
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApmFavorites {
    services: Vec<FavoriteService>,
}

fn validate_apm_favorites(value: &Value) -> Result<()> {
    let favorites: ApmFavorites = serde_json::from_value(value.clone())
        .map_err(|error| Error::invalid(format!("APM favorites: {error}")))?;
    let services = &favorites.services;

    let shaped = services.len() <= MAX_APM_FAVORITES
        && services.iter().enumerate().all(|(index, service)| {
            (1..=128).contains(&service.namespace.chars().count())
                && (1..=MAX_KEY_CHARS).contains(&service.name.chars().count())
                && !services[..index].contains(service)
        });

    if !shaped {
        return Err(Error::invalid(format!(
            "APM favorites: at most {MAX_APM_FAVORITES} services, each once by namespace and name"
        )));
    }

    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NotificationsRead {
    ids: Vec<String>,
}

fn validate_notifications_read(value: &Value) -> Result<()> {
    let read: NotificationsRead = serde_json::from_value(value.clone())
        .map_err(|error| Error::invalid(format!("read notifications: {error}")))?;

    let shaped = read.ids.len() <= MAX_READ_NOTIFICATIONS
        && read
            .ids
            .iter()
            .all(|id| (1..=MAX_NOTIFICATION_ID_CHARS).contains(&id.chars().count()));

    if !shaped {
        return Err(Error::invalid(format!(
            "read notifications: at most {MAX_READ_NOTIFICATIONS} ids of 1 to {MAX_NOTIFICATION_ID_CHARS} characters"
        )));
    }

    Ok(())
}

/// The width of a drawer opened over a list: an endpoint, an error group, a log.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DrawerLayout {
    drawer: Option<u32>,
}

fn validate_drawer_layout(value: &Value, what: &str) -> Result<()> {
    let layout: DrawerLayout = serde_json::from_value(value.clone())
        .map_err(|error| Error::invalid(format!("{what}: {error}")))?;

    if layout
        .drawer
        .is_some_and(|width| !TRACE_LAYOUT_WIDTHS.contains(&width))
    {
        return Err(Error::invalid(format!(
            "{what}: a width is 160 to 4000 pixels"
        )));
    }

    Ok(())
}

fn validate_key(key: &str) -> Result<()> {
    if key.trim().is_empty() || key.chars().count() > MAX_KEY_CHARS {
        return Err(Error::invalid(
            "interface preference key is 1 to 255 characters",
        ));
    }

    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UiPreferenceResp {
    /// `null` where nothing is kept: the page then shows what it would by default.
    value: Option<Value>,
    updated_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteReq {
    value: Value,
}

async fn read(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path((scope, key)): Path<(String, String)>,
) -> Result<Json<UiPreferenceResp>> {
    let scope = Scope::parse(&scope)?;
    validate_key(&key)?;

    let kept = state
        .iam
        .user_ui_preference
        .get(UiPreferenceKey {
            user_id: &ctx.user_id,
            org_id: &ctx.org_id,
            scope: scope.as_str(),
            key: &key,
        })
        .await?;

    Ok(Json(match kept {
        Some(kept) => UiPreferenceResp {
            value: Some(kept.value),
            updated_at: Some(kept.updated_at),
        },
        None => UiPreferenceResp {
            value: None,
            updated_at: None,
        },
    }))
}

async fn write(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path((scope, key)): Path<(String, String)>,
    Json(request): Json<WriteReq>,
) -> Result<Json<UiPreferenceResp>> {
    let scope = Scope::parse(&scope)?;
    validate_key(&key)?;
    scope.validate(&request.value)?;

    if request.value.to_string().len() > MAX_VALUE_BYTES {
        return Err(Error::invalid("interface preference is too large"));
    }

    let kept = state
        .iam
        .user_ui_preference
        .put(
            UiPreferenceKey {
                user_id: &ctx.user_id,
                org_id: &ctx.org_id,
                scope: scope.as_str(),
                key: &key,
            },
            request.value,
        )
        .await?;

    Ok(Json(UiPreferenceResp {
        value: Some(kept.value),
        updated_at: Some(kept.updated_at),
    }))
}

async fn clear(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path((scope, key)): Path<(String, String)>,
) -> Result<StatusCode> {
    let scope = Scope::parse(&scope)?;
    validate_key(&key)?;

    state
        .iam
        .user_ui_preference
        .delete(UiPreferenceKey {
            user_id: &ctx.user_id,
            org_id: &ctx.org_id,
            scope: scope.as_str(),
            key: &key,
        })
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests;
