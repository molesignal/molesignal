// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::{
    ToolInvocationContext, ToolResult,
    catalog::{BuiltinToolKind, tools_for_surface},
};

use super::{ToolRuntime, common::parse_args};
use crate::{app::iam::IamContext, shared::Result};

pub(super) async fn execute(
    _runtime: &ToolRuntime,
    _auth: &IamContext,
    ctx: &ToolInvocationContext,
    kind: BuiltinToolKind,
    arguments: Value,
) -> Result<ToolResult> {
    match kind {
        BuiltinToolKind::GetPlatformCapabilities => {
            let _: EmptyArgs = parse_args(arguments)?;
            let mut domains = BTreeMap::<String, Vec<Value>>::new();
            for tool in tools_for_surface(ctx.source().surface()) {
                domains.entry(tool.domain.clone()).or_default().push(json!({
                    "name": tool.name,
                    "category": tool.category,
                    "access": tool.access,
                    "risk": tool.risk,
                    "required_permissions": tool.required_permissions,
                    "permission_mode": tool.permission_mode,
                    "exposure": tool.exposure,
                }));
            }
            Ok(ToolResult::json(json!({
                "builtin_tool_count": domains.values().map(Vec::len).sum::<usize>(),
                "surface": ctx.source().surface(),
                "domains": domains,
                "directly_excluded_entrypoints": [
                    "authentication and password reset",
                    "credential or secret material",
                    "raw intake protocols and external webhooks",
                    "binary file transfer",
                    "node drain and runtime profiling",
                    "public unauthenticated routes"
                ],
                "mutation_policy": {
                    "automatic": "create an auditable approval record and execute immediately",
                    "confirmation": "create an approved request and wait for explicit execution",
                    "single_approval": "execute automatically after the final required approval",
                    "dual_approval": "execute automatically after two distinct approvals",
                    "disabled": "reject the operation before creating an approval request"
                }
            })))
        }
        _ => unreachable!("platform handler received unrelated tool"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyArgs {}
