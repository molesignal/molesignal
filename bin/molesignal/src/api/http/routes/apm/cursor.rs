// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Signed, query-bound cursor transport for APM ranking lists.

use serde::{Deserialize, Serialize};

use super::QueryParams;
use crate::{
    api::{
        AppState,
        http::pagination::cursor::{decode_signed_cursor, encode_signed_cursor},
    },
    app::{
        apm::{ApmQueryContext, ApmQueryRequest, PagedResponse},
        iam::IamContext,
    },
    domain::apm::{QueryResolution, SortDirection},
    shared::{Error, Result},
};

pub(super) const SERVICES: &str = "apm.services.v1";
pub(super) const TRANSACTIONS: &str = "apm.transactions.v1";
pub(super) const DEPENDENCIES: &str = "apm.dependencies.v1";
pub(super) const ERRORS: &str = "apm.errors.v1";

const VERSION: u8 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ApmCursorPayload {
    version: u8,
    from: i64,
    to: i64,
    namespace: Option<String>,
    service: Option<String>,
    environment: Option<String>,
    release: Option<String>,
    resolution: QueryResolution,
    sort: String,
    direction: SortDirection,
    page_size: usize,
    inner_cursor: String,
}

pub(super) fn request(
    state: &AppState,
    iam: &IamContext,
    params: &QueryParams,
    purpose: &str,
) -> Result<ApmQueryRequest> {
    let Some(token) = params.cursor.as_deref() else {
        return Ok(params.request());
    };
    let payload = decode_signed_cursor::<ApmCursorPayload>(
        state.iam.service.as_ref(),
        &iam.org_id,
        purpose,
        token,
    )?;
    if payload.version != VERSION
        || payload.to < payload.from
        || payload.inner_cursor.is_empty()
        || !(1..=200).contains(&payload.page_size)
    {
        return Err(Error::invalid("invalid APM cursor"));
    }
    validate_request(params, &payload)?;
    Ok(ApmQueryRequest {
        from: payload.from,
        to: payload.to,
        namespace: payload.namespace,
        service: payload.service,
        environment: payload.environment,
        version: payload.release,
        resolution: payload.resolution,
        sort: Some(payload.sort),
        direction: Some(payload.direction),
        limit: Some(payload.page_size),
        cursor: Some(payload.inner_cursor),
    })
}

pub(super) fn sign_page<T>(
    state: &AppState,
    iam: &IamContext,
    purpose: &str,
    context: &ApmQueryContext,
    mut response: PagedResponse<T>,
) -> Result<PagedResponse<T>> {
    response.previous_cursor = response
        .previous_cursor
        .take()
        .map(|inner| encode(state, iam, purpose, context, inner))
        .transpose()?;
    response.next_cursor = response
        .next_cursor
        .take()
        .map(|inner| encode(state, iam, purpose, context, inner))
        .transpose()?;
    response.has_more = response.next_cursor.is_some();
    Ok(response)
}

fn encode(
    state: &AppState,
    iam: &IamContext,
    purpose: &str,
    context: &ApmQueryContext,
    inner_cursor: String,
) -> Result<String> {
    encode_signed_cursor(
        state.iam.service.as_ref(),
        &iam.org_id,
        purpose,
        ApmCursorPayload {
            version: VERSION,
            from: context.range.start.0,
            to: context.range.end.0,
            namespace: context.namespace.clone(),
            service: context.service_name.clone(),
            environment: context.environment.clone(),
            release: context.version.clone(),
            resolution: context.resolution,
            sort: context.sort.clone(),
            direction: context.direction,
            page_size: context.limit,
            inner_cursor,
        },
    )
}

fn validate_request(params: &QueryParams, cursor: &ApmCursorPayload) -> Result<()> {
    let mismatch = params.from.is_some_and(|value| value != cursor.from)
        || params.to.is_some_and(|value| value != cursor.to)
        || differs(&params.namespace, &cursor.namespace)
        || differs(&params.service, &cursor.service)
        || differs(&params.environment, &cursor.environment)
        || differs(&params.version, &cursor.release)
        || params
            .resolution
            .is_some_and(|value| value != QueryResolution::Auto && value != cursor.resolution)
        || params
            .sort
            .as_ref()
            .is_some_and(|value| value != &cursor.sort)
        || params
            .direction
            .is_some_and(|value| value != cursor.direction)
        || params.limit.is_some_and(|value| value != cursor.page_size);
    if mismatch {
        return Err(Error::invalid("APM cursor does not match active query"));
    }
    Ok(())
}

fn differs(requested: &Option<String>, frozen: &Option<String>) -> bool {
    requested.as_ref().is_some_and(|value| {
        Some(value.trim()).filter(|value| !value.is_empty()) != frozen.as_deref()
    })
}
