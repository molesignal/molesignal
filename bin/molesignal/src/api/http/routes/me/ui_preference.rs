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
}

impl Scope {
    fn parse(raw: &str) -> Result<Self> {
        match raw {
            "logs.columns" => Ok(Self::LogColumns),
            _ => Err(Error::invalid(format!(
                "unknown interface preference: {raw}"
            ))),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::LogColumns => "logs.columns",
        }
    }

    /// Whether `value` is a preference of this purpose.
    fn validate(self, value: &Value) -> Result<()> {
        match self {
            Self::LogColumns => validate_log_columns(value),
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

fn validate_key(key: &str) -> Result<()> {
    if key.trim().is_empty() || key.chars().count() > MAX_KEY_CHARS {
        return Err(Error::invalid(
            "interface preference key is 1 to 255 characters",
        ));
    }

    Ok(())
}

#[derive(Debug, Serialize)]
struct UiPreferenceResp {
    /// `null` where nothing is kept: the page then shows what it would by default.
    value: Option<Value>,
    updated_at_micros: Option<i64>,
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
            updated_at_micros: Some(kept.updated_at_micros),
        },
        None => UiPreferenceResp {
            value: None,
            updated_at_micros: None,
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
        updated_at_micros: Some(kept.updated_at_micros),
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
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_purpose_is_one_this_product_knows() {
        assert_eq!(Scope::parse("logs.columns").unwrap(), Scope::LogColumns);
        assert_eq!(Scope::LogColumns.as_str(), "logs.columns");
        assert!(Scope::parse("logs").is_err());
        assert!(Scope::parse("anything.else").is_err());
    }

    #[test]
    fn log_columns_are_field_names_in_an_order() {
        let scope = Scope::LogColumns;

        assert!(
            scope
                .validate(&json!({ "fields": ["level", "service.name", "message"] }))
                .is_ok()
        );
        // No column beside the time is an arrangement too.
        assert!(scope.validate(&json!({ "fields": [] })).is_ok());
    }

    #[test]
    fn log_columns_are_refused_in_any_other_shape() {
        let scope = Scope::LogColumns;

        assert!(scope.validate(&json!(["level"])).is_err());
        assert!(scope.validate(&json!({ "fields": "level" })).is_err());
        assert!(
            scope
                .validate(&json!({ "fields": ["level"], "widths": {} }))
                .is_err()
        );
        assert!(scope.validate(&json!({ "fields": ["level", 3] })).is_err());
        assert!(scope.validate(&json!({ "fields": [""] })).is_err());
        assert!(
            scope
                .validate(&json!({ "fields": ["level", "level"] }))
                .is_err()
        );
        assert!(
            scope
                .validate(&json!({ "fields": ["x".repeat(256)] }))
                .is_err()
        );

        let many: Vec<String> = (0..=MAX_LOG_COLUMNS)
            .map(|index| format!("f{index}"))
            .collect();
        assert!(scope.validate(&json!({ "fields": many })).is_err());
    }

    #[test]
    fn a_key_is_a_name_of_reasonable_length() {
        assert!(validate_key("application_logs").is_ok());
        assert!(validate_key("k8s/events.prod").is_ok());
        assert!(validate_key("").is_err());
        assert!(validate_key("   ").is_err());
        assert!(validate_key(&"k".repeat(256)).is_err());
    }
}
