// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use crate::{
    api::http::pagination::cursor::{CursorDirection, decode_signed_cursor, encode_signed_cursor},
    app::iam::IamService,
    shared::{Error, Result, ids::Id},
};

const VERSION: u8 = 1;
const PURPOSE: &str = "metrics.catalog.v1";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct MetricCatalogCursor {
    version: u8,
    pub(super) query: Option<String>,
    pub(super) page_size: usize,
    pub(super) direction: CursorDirection,
    pub(super) metric_name: String,
}

pub(super) fn decode(iam: &IamService, org_id: &Id, token: &str) -> Result<MetricCatalogCursor> {
    let payload = decode_signed_cursor::<MetricCatalogCursor>(iam, org_id, PURPOSE, token)?;
    if payload.version != VERSION
        || payload.metric_name.is_empty()
        || payload.metric_name.len() > 512
        || !(1..=super::MAX_PAGE_SIZE).contains(&payload.page_size)
    {
        return Err(Error::invalid("invalid metric catalog cursor"));
    }
    Ok(payload)
}

pub(super) fn encode(
    iam: &IamService,
    org_id: &Id,
    query: Option<String>,
    page_size: usize,
    direction: CursorDirection,
    metric_name: String,
) -> Result<String> {
    encode_signed_cursor(
        iam,
        org_id,
        PURPOSE,
        MetricCatalogCursor {
            version: VERSION,
            query,
            page_size,
            direction,
            metric_name,
        },
    )
}
