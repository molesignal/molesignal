// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::{HashMap, HashSet};

use axum::{Extension, Json, Router, extract::State, routing::get};
use serde::Serialize;

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{
        iam::{UserStatus, permission},
        notify::{endpoint::UserNotifyEndpoint, preference::UserNotifyPreference},
    },
    shared::{Result, ids::Id},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/notify/users", get(list))
}

#[derive(Debug, Serialize)]
struct NotifyUserSummary {
    user_id: Id,
    email: String,
    display_name: String,
    avatar_url: Option<String>,
    disabled: bool,
    status: UserStatus,
    endpoints: Vec<UserNotifyEndpoint>,
    preferences: Vec<UserNotifyPreference>,
}

#[permission("org.members.read")]
async fn list(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
) -> Result<Json<Vec<NotifyUserSummary>>> {
    let member_ids = state
        .iam
        .service
        .iam_memberships
        .list_for_org(&context.org_id)
        .await?
        .into_iter()
        .map(|membership| membership.user_id)
        .collect::<HashSet<_>>();
    let mut endpoints = group_by_user(
        state
            .alerting
            .notify
            .list_organization_endpoints(&context.org_id)
            .await?,
        |endpoint| endpoint.user_id.clone(),
    );
    let mut preferences = group_by_user(
        state
            .alerting
            .notify
            .list_organization_preferences(&context.org_id)
            .await?,
        |preference| preference.user_id.clone(),
    );
    let mut users = state
        .iam
        .service
        .users
        .list()
        .await?
        .into_iter()
        .filter(|user| member_ids.contains(&user.id))
        .map(|user| NotifyUserSummary {
            endpoints: endpoints.remove(&user.id).unwrap_or_default(),
            preferences: preferences.remove(&user.id).unwrap_or_default(),
            user_id: user.id,
            email: user.email,
            display_name: user.display_name,
            avatar_url: user.avatar_url,
            disabled: user.disabled,
            status: user.status,
        })
        .collect::<Vec<_>>();
    users.sort_by(|left, right| {
        left.display_name
            .to_lowercase()
            .cmp(&right.display_name.to_lowercase())
            .then_with(|| left.email.cmp(&right.email))
    });
    Ok(Json(users))
}

fn group_by_user<T>(values: Vec<T>, user_id: impl Fn(&T) -> Id) -> HashMap<Id, Vec<T>> {
    let mut grouped = HashMap::new();
    for value in values {
        grouped
            .entry(user_id(&value))
            .or_insert_with(Vec::new)
            .push(value);
    }
    grouped
}
