// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Notify HTTP 控制面。

mod connectors;
mod defaults;
mod deliveries;
mod endpoints;
mod policies;
mod preferences;
mod templates;
mod users;

use axum::Router;

use crate::{
    api::AppState,
    app::iam::IamContext,
    shared::{Error, Result, ids::Id},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(connectors::routes())
        .merge(defaults::routes())
        .merge(endpoints::routes())
        .merge(policies::routes())
        .merge(preferences::routes())
        .merge(templates::routes())
        .merge(users::routes())
        .merge(deliveries::routes())
}

pub(super) fn parse_category(
    value: &str,
) -> Result<crate::domain::notify::preference::NotifyCategory> {
    use crate::domain::notify::preference::NotifyCategory;
    match value {
        "alert" => Ok(NotifyCategory::Alert),
        "oncall" => Ok(NotifyCategory::Oncall),
        "escalation" => Ok(NotifyCategory::Escalation),
        "report" => Ok(NotifyCategory::Report),
        "security" => Ok(NotifyCategory::Security),
        "system" => Ok(NotifyCategory::System),
        _ => Err(Error::invalid("unknown notify category")),
    }
}

pub(super) async fn ensure_user_scope(
    state: &AppState,
    context: &IamContext,
    user_id: &Id,
    write: bool,
) -> Result<()> {
    if &context.user_id == user_id {
        return Ok(());
    }
    let permission = if write {
        "org.members.manage"
    } else {
        "org.members.read"
    };
    if !context.has_permission(permission) {
        return Err(Error::forbidden(format!(
            "{permission} permission is required for another user's notify settings"
        )));
    }
    let is_member = state
        .iam
        .service
        .iam_memberships
        .list_for_org(&context.org_id)
        .await?
        .into_iter()
        .any(|membership| membership.user_id == *user_id);
    if !is_member {
        return Err(Error::not_found("organization user"));
    }
    Ok(())
}
