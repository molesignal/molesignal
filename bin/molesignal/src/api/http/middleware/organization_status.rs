// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A disabled organization is closed. Every credential of it, a signed-in session or an API token,
//! answers 403 at once, except on what gets someone out of it: signing out, the account itself, the
//! list of organizations, and switching to another one.
//!
//! Sits inside `auth_layer`, to read the [`IamContext`] it adds. A request on the auth whitelist has
//! none and passes.

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};

use crate::{api::AppState, app::iam::IamContext, shared::Error};

/// Answered whatever the state of the organization: each with what lies under it.
const RECOVERY_PATHS: &[&str] = &[
    "/api/v1/auth",
    "/api/v1/healthz",
    "/api/v1/me",
    "/api/v1/instance",
    "/api/v1/iam/capabilities",
];

fn is_recovery_path(path: &str) -> bool {
    let under = |parent: &str| {
        path.strip_prefix(parent)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    };

    if RECOVERY_PATHS.iter().any(|parent| under(parent)) || path == "/api/v1/orgs" {
        return true;
    }

    let Some(rest) = path.strip_prefix("/api/v1/orgs/") else {
        return false;
    };

    let mut segments = rest.split('/');

    matches!(
        (segments.next(), segments.next(), segments.next()),
        (Some(id), Some("select"), None) if !id.is_empty()
    )
}

pub async fn organization_status_layer(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, Error> {
    let Some(organization) = req
        .extensions()
        .get::<IamContext>()
        .map(|context| context.org_id.clone())
    else {
        return Ok(next.run(req).await);
    };

    if !is_recovery_path(req.uri().path()) {
        state
            .iam
            .service
            .ensure_organization_access(&organization)
            .await?;
    }

    Ok(next.run(req).await)
}

#[cfg(test)]
mod tests {
    use super::is_recovery_path;

    #[test]
    fn a_closed_organization_answers_only_what_gets_someone_out_of_it() {
        for open in [
            "/api/v1/auth/signout",
            "/api/v1/me",
            "/api/v1/me/profile",
            "/api/v1/iam/capabilities",
            "/api/v1/instance",
            "/api/v1/orgs",
            "/api/v1/orgs/tenant-a/select",
        ] {
            assert!(is_recovery_path(open), "{open}");
        }

        for closed in [
            "/api/v1/dashboards",
            "/api/v1/mcp",
            "/api/v1/mechanics",
            "/api/v1/orgs/tenant-a",
            "/api/v1/orgs/tenant-a/status",
            "/api/v1/orgs/tenant-a/select/extra",
            "/api/v1/orgs//select",
            "/api/v1/iam/role-bindings",
        ] {
            assert!(!is_recovery_path(closed), "{closed}");
        }
    }
}
