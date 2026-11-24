// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What services run on: clusters, nodes, hosts, pods and containers, and the services on each.

use serde_json::json;

use super::{BuiltinToolKind, object_schema, open_output};
use crate::ToolSpec;

const KINDS: [&str; 5] = ["cluster", "node", "host", "pod", "container"];

pub(super) fn spec(kind: BuiltinToolKind) -> ToolSpec {
    let (description, input, tags) = match kind {
        BuiltinToolKind::ListEntities => (
            "List what services run on, seen over the last week: Kubernetes clusters, nodes and pods, hosts outside Kubernetes, and containers, each with its key (a node is `cluster/node`, a pod `cluster/namespace/pod`), its resource attributes, when it was first and last seen, and how many services run on it directly. They are read every five minutes from the instances APM sees and the resource fields of metrics and logs.",
            object_schema(json!({
                "kind": {"type": "string", "enum": KINDS},
                "search": {"type": "string", "description": "Part of the name or the key."},
                "limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 50}
            })),
            vec!["Infrastructure", "Entities"],
        ),
        BuiltinToolKind::GetEntity => (
            "Return one cluster, node, host, pod or container: its attributes; what it runs on or is part of (a pod's node, a node's cluster); what runs on or is part of it; and the services on it, those on its nodes and pods included. Call it to learn which services share a node or host that is in trouble, or where a service's instances run.",
            json!({
                "type": "object", "required": ["kind", "key"],
                "properties": {
                    "kind": {"type": "string", "enum": KINDS},
                    "key": {"type": "string", "description": "The key list_entities gives."}
                }, "additionalProperties": false
            }),
            vec!["Infrastructure", "Entities"],
        ),
        _ => unreachable!("entity catalog received unrelated kind"),
    };

    ToolSpec::read(
        kind.name(),
        description,
        "observability",
        "infrastructure",
        input,
        open_output(),
        &[
            "apm.services.read",
            "metrics.read",
            "logs.read",
            "sys.telemetry.read",
        ],
        &tags,
    )
    .any_permission()
}
