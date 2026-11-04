// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The caller's read grants travel with the request: a caller with `audit.read` reads the audit
//! trail (`_audit`) in the queries the request makes, a caller without it does not. The query
//! engine reads them ([`crate::infra::query::grants`]); inside the authentication layer, where
//! the request already carries its [`IamContext`].

use axum::{extract::Request, middleware::Next, response::Response};

use crate::{
    app::iam::IamContext,
    infra::query::grants::{AUDIT_READ, ReadGrants, scope},
};

pub async fn read_grants_layer(req: Request, next: Next) -> Response {
    let grants = req
        .extensions()
        .get::<IamContext>()
        .map(|ctx| ReadGrants::of_caller(ctx.has_permission(AUDIT_READ)))
        .unwrap_or_default();

    scope(grants, next.run(req)).await
}
