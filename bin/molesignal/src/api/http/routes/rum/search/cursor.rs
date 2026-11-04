// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The signed cursor of a page of the explorer. It carries the whole query, so a page is cut from
//! the same rows however far on the reader has gone, and the boundary row.

use serde::{Deserialize, Serialize};

use super::Context;
use crate::{
    api::http::pagination::cursor::{CursorDirection, decode_signed_cursor, encode_signed_cursor},
    app::iam::IamService,
    infra::rum::query::Filter,
    shared::{Error, Result, ids::Id},
};

const VERSION: u8 = 1;
const PURPOSE: &str = "rum.search.v1";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Payload {
    version: u8,
    pub collection: String,
    pub from: i64,
    pub to: i64,
    pub page_size: usize,
    pub filters: Vec<Filter>,
    pub free_text: Vec<String>,
    pub columns: Vec<String>,
    pub direction: CursorDirection,
    pub timestamp: i64,
    pub tie: String,
}

pub(super) fn decode(iam: &IamService, org_id: &Id, token: &str) -> Result<Payload> {
    let payload = decode_signed_cursor::<Payload>(iam, org_id, PURPOSE, token)?;

    if payload.version != VERSION
        || payload.to <= payload.from
        || !(1..=super::MAX_PAGE_SIZE).contains(&payload.page_size)
        || payload.tie.len() > 256
    {
        return Err(Error::invalid("invalid RUM cursor payload"));
    }

    Ok(payload)
}

/// The cursor of the page that follows or precedes `context`, bounded by the row at `at`.
pub(super) fn encode(
    iam: &IamService,
    org_id: &Id,
    context: &Context,
    direction: CursorDirection,
    at: (i64, &str),
) -> Result<String> {
    encode_signed_cursor(
        iam,
        org_id,
        PURPOSE,
        Payload {
            version: VERSION,
            collection: context.selection.stream.collection().to_string(),
            from: context.selection.span.from,
            to: context.selection.span.to,
            page_size: context.page_size,
            filters: context.selection.filters.clone(),
            free_text: context.selection.free_text.clone(),
            columns: context.columns.clone(),
            direction,
            timestamp: at.0,
            tie: at.1.to_string(),
        },
    )
}
