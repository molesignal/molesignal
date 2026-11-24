// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use domain::{
    entity::{EntityKind, EntityQuery},
    iam::IamContext,
};
use kernel::{Error, Result, time::TimestampMicros};
use serde::Deserialize;
use serde_json::Value;
use tool_runtime::{ToolResult, catalog::BuiltinToolKind};

use super::{ToolRuntime, common::parse_args};
use crate::app::entities::SWEEP_WINDOW_MICROS;

const DEFAULT_LIMIT: usize = 50;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListArgs {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    search: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetArgs {
    kind: String,
    key: String,
}

fn kind_of(kind: &str) -> Result<EntityKind> {
    EntityKind::parse(kind).ok_or_else(|| Error::invalid(format!("unknown entity kind `{kind}`")))
}

pub(super) async fn execute(
    runtime: &ToolRuntime,
    auth: &IamContext,
    kind: BuiltinToolKind,
    arguments: Value,
) -> Result<ToolResult> {
    let entities = &runtime.observability.entities;

    let value = match kind {
        BuiltinToolKind::ListEntities => {
            let args: ListArgs = parse_args(arguments)?;
            let query = EntityQuery {
                kind: args.kind.as_deref().map(kind_of).transpose()?,
                search: args.search,
                seen_since: None,
                limit: args.limit.unwrap_or(DEFAULT_LIMIT),
            };

            serde_json::to_value(entities.list(&auth.org_id, &query).await?)
        }
        BuiltinToolKind::GetEntity => {
            let args: GetArgs = parse_args(arguments)?;

            let Some(detail) = entities
                .detail(&auth.org_id, kind_of(&args.kind)?, &args.key)
                .await?
            else {
                return Err(Error::not_found(format!(
                    "no {} `{}` was seen; list_entities gives the keys",
                    args.kind, args.key
                )));
            };

            // Seen within a sweep or two is running now; older is what was last seen.
            let current =
                detail.entity.last_seen_at.0 >= TimestampMicros::now().0 - 2 * SWEEP_WINDOW_MICROS;
            let mut value =
                serde_json::to_value(detail).map_err(|error| Error::internal(error.to_string()))?;

            if let Some(object) = value.as_object_mut() {
                object.insert("current".into(), current.into());
            }

            Ok(value)
        }
        _ => unreachable!("entity handler received unrelated tool"),
    }
    .map_err(|error| Error::internal(error.to_string()))?;

    Ok(ToolResult::json(value))
}
