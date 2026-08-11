// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};

use crate::{
    domain::apm::{QueryCursor, QueryResolution, SortDirection},
    shared::{
        Error, Result,
        cursor::CursorDirection,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

const MAX_FILTER_BYTES: usize = 192;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ApmQueryRequest {
    pub from: i64,
    pub to: i64,
    pub namespace: Option<String>,
    pub service: Option<String>,
    pub environment: Option<String>,
    pub version: Option<String>,
    #[serde(default)]
    pub resolution: QueryResolution,
    pub sort: Option<String>,
    pub direction: Option<SortDirection>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ApmQueryContext {
    pub org_id: Id,
    pub range: TimeRange,
    pub namespace: Option<String>,
    pub service_name: Option<String>,
    pub environment: Option<String>,
    pub version: Option<String>,
    pub resolution: QueryResolution,
    pub sort: String,
    pub direction: SortDirection,
    pub limit: usize,
    pub cursor: Option<QueryCursor>,
}

impl ApmQueryContext {
    pub fn build(
        org_id: Id,
        request: ApmQueryRequest,
        max_range_micros: i64,
        hot_resolution_micros: i64,
        allowed_sorts: &[&str],
        default_sort: &str,
    ) -> Result<Self> {
        if request.to < request.from {
            return Err(Error::invalid("APM range end precedes start"));
        }
        let range = TimeRange::new(TimestampMicros(request.from), TimestampMicros(request.to));
        if range.duration_micros() > max_range_micros {
            return Err(Error::invalid("APM query range exceeds configured maximum"));
        }
        let resolution = match request.resolution {
            QueryResolution::Auto if range.duration_micros() <= hot_resolution_micros => {
                QueryResolution::Minute
            }
            QueryResolution::Auto => QueryResolution::Hour,
            explicit => explicit,
        };
        let sort = request.sort.unwrap_or_else(|| default_sort.to_owned());
        if !allowed_sorts.contains(&sort.as_str()) {
            return Err(Error::invalid("unsupported APM sort field"));
        }
        let direction = request.direction.unwrap_or(SortDirection::Desc);
        let limit = request.limit.unwrap_or(50);
        if !(1..=200).contains(&limit) {
            return Err(Error::invalid("APM page limit must be between 1 and 200"));
        }
        let cursor = request.cursor.as_deref().map(decode_cursor).transpose()?;
        if cursor.as_ref().is_some_and(|cursor| {
            cursor.version != 1 || cursor.sort_field != sort || cursor.direction != direction
        }) {
            return Err(Error::invalid("APM cursor does not match active sort"));
        }
        Ok(Self {
            org_id,
            range,
            namespace: validate_filter("namespace", request.namespace)?,
            service_name: validate_filter("service", request.service)?,
            environment: validate_filter("environment", request.environment)?,
            version: validate_filter("version", request.version)?,
            resolution,
            sort,
            direction,
            limit,
            cursor,
        })
    }

    pub fn encode_cursor(
        &self,
        page_direction: CursorDirection,
        sort_value: String,
        tie_breaker: String,
    ) -> Result<String> {
        let bytes = serde_json::to_vec(&QueryCursor {
            version: 1,
            sort_field: self.sort.clone(),
            direction: self.direction,
            page_direction,
            sort_value,
            tie_breaker,
        })
        .map_err(|_| Error::internal("encode APM cursor"))?;
        Ok(URL_SAFE_NO_PAD.encode(bytes))
    }
}

fn decode_cursor(value: &str) -> Result<QueryCursor> {
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| Error::invalid("invalid APM cursor encoding"))?;
    serde_json::from_slice(&bytes).map_err(|_| Error::invalid("invalid APM cursor payload"))
}

fn validate_filter(name: &str, value: Option<String>) -> Result<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() || value.len() > MAX_FILTER_BYTES || value.contains('\0') {
        return Err(Error::invalid(format!("invalid APM {name} filter")));
    }
    Ok(Some(value.to_owned()))
}

#[derive(Debug, Clone, Serialize)]
pub struct ApmQueryRange {
    pub from: TimestampMicros,
    pub to: TimestampMicros,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_validates_range_filter_sort_and_cursor_round_trip() {
        let context = ApmQueryContext::build(
            Id::from_string("org"),
            ApmQueryRequest {
                from: 0,
                to: 60_000_000,
                sort: Some("request_count".into()),
                direction: Some(SortDirection::Desc),
                ..ApmQueryRequest::default()
            },
            1_000_000_000,
            86_400_000_000,
            &["request_count"],
            "request_count",
        )
        .expect("context");
        let cursor = context
            .encode_cursor(CursorDirection::After, "10".into(), "service".into())
            .expect("cursor");
        let decoded = decode_cursor(&cursor).expect("decode");
        assert_eq!(decoded.sort_field, "request_count");
        assert_eq!(decoded.tie_breaker, "service");
    }

    #[test]
    fn context_rejects_mismatched_cursor_and_oversized_range() {
        let context = ApmQueryContext::build(
            Id::from_string("org"),
            ApmQueryRequest {
                from: 0,
                to: 10,
                ..ApmQueryRequest::default()
            },
            10,
            10,
            &["name"],
            "name",
        )
        .expect("context");
        let cursor = context
            .encode_cursor(CursorDirection::After, "x".into(), "x".into())
            .expect("cursor");
        assert!(
            ApmQueryContext::build(
                Id::from_string("org"),
                ApmQueryRequest {
                    from: 0,
                    to: 11,
                    cursor: Some(cursor),
                    ..ApmQueryRequest::default()
                },
                10,
                10,
                &["request_count"],
                "request_count",
            )
            .is_err()
        );
    }
}
