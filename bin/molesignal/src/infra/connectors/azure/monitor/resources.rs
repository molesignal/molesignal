// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Which resources a pull reads: those of the resource type in the region and subscription, as
//! Azure Resource Graph lists them, with their resource group and tags for labels.

use serde_json::{Value, json};

use super::settings::Settings;

/// The Resource Graph endpoint.
pub(super) const RESOURCE_GRAPH_URL: &str = "https://management.azure.com/providers/Microsoft.ResourceGraph/resources?api-version=2022-10-01";

/// The most rows Resource Graph returns on one page.
pub(super) const PAGE_SIZE: u32 = 1_000;

/// One resource whose metrics are read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Resource {
    /// The full resource ID, `/subscriptions/…/resourceGroups/…/providers/…`.
    pub id: String,
    pub name: String,
    pub resource_group: String,
    /// Tags, by key.
    pub tags: Vec<(String, String)>,
}

/// One page of resources and where the next one starts.
#[derive(Debug, Default)]
pub(super) struct Page {
    pub resources: Vec<Resource>,
    pub skip_token: Option<String>,
}

/// `value` as a KQL string literal: in single quotes, with backslashes and quotes escaped.
pub(super) fn kql_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);

    out.push('\'');

    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' | '\r' => out.push(' '),
            _ => out.push(ch),
        }
    }

    out.push('\'');
    out
}

/// The query listing the resources: ordered by ID, so the pages hold the same ones each time.
pub(super) fn query(settings: &Settings) -> String {
    format!(
        "Resources | where type =~ {} and location =~ {} | project id, name, resourceGroup, tags \
         | order by id asc",
        kql_string(&settings.resource_type),
        kql_string(&settings.region)
    )
}

/// The body of the request for one page of at most `top` rows.
pub(super) fn request_body(settings: &Settings, top: u32, skip_token: Option<&str>) -> Value {
    let mut options = json!({ "$top": top, "resultFormat": "objectArray" });

    if let Some(token) = skip_token {
        options["$skipToken"] = Value::String(token.to_string());
    }

    json!({
        "subscriptions": [settings.subscription_id],
        "query": query(settings),
        "options": options,
    })
}

/// A Resource Graph response as a page of resources.
pub(super) fn parse_page(body: &Value) -> Page {
    let resources = body
        .get("data")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().filter_map(resource).collect())
        .unwrap_or_default();

    let skip_token = body
        .get("$skipToken")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .map(str::to_string);

    Page {
        resources,
        skip_token,
    }
}

fn resource(row: &Value) -> Option<Resource> {
    let id = row.get("id").and_then(Value::as_str)?.to_string();
    let field = |key: &str| row.get(key).and_then(Value::as_str).map(str::to_string);

    let mut tags: Vec<(String, String)> = row
        .get("tags")
        .and_then(Value::as_object)
        .map(|tags| {
            tags.iter()
                .map(|(key, value)| {
                    let value = match value {
                        Value::String(text) => text.clone(),
                        other => other.to_string(),
                    };

                    (key.clone(), value)
                })
                .collect()
        })
        .unwrap_or_default();

    tags.sort();

    Some(Resource {
        name: field("name").unwrap_or_else(|| segment_after(&id, "providers").unwrap_or_default()),
        resource_group: field("resourceGroup")
            .unwrap_or_else(|| segment_after(&id, "resourceGroups").unwrap_or_default()),
        id,
        tags,
    })
}

/// The part of a resource ID after `key` (`resourceGroups` → the group); for `providers`, the
/// last part, the resource's name.
pub(super) fn segment_after(id: &str, key: &str) -> Option<String> {
    let parts: Vec<&str> = id.split('/').filter(|part| !part.is_empty()).collect();

    if key == "providers" {
        return parts.last().map(|name| name.to_string());
    }

    parts
        .iter()
        .position(|part| part.eq_ignore_ascii_case(key))
        .and_then(|at| parts.get(at + 1))
        .map(|part| part.to_string())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn settings() -> Settings {
        Settings::parse(&json!({
            "tenantId": "t",
            "clientId": "c",
            "clientSecret": "s",
            "subscriptionId": "cfbbd179-59d2-4052-aa06-9270a38aa9d6",
            "region": "eastus",
            "resourceType": "Microsoft.Compute/virtualMachines",
            "metrics": ["Percentage CPU"],
        }))
        .unwrap()
    }

    #[test]
    fn strings_in_the_query_are_escaped() {
        assert_eq!(kql_string("a'b\\c"), r"'a\'b\\c'");
        assert_eq!(
            query(&settings()),
            "Resources | where type =~ 'Microsoft.Compute/virtualMachines' and location =~ \
             'eastus' | project id, name, resourceGroup, tags | order by id asc"
        );
    }

    #[test]
    fn the_next_page_is_asked_for_with_its_token() {
        let first = request_body(&settings(), PAGE_SIZE, None);

        assert_eq!(
            first["subscriptions"],
            json!(["cfbbd179-59d2-4052-aa06-9270a38aa9d6"])
        );
        assert_eq!(first["options"]["$top"], 1000);
        assert!(first["options"].get("$skipToken").is_none());

        let next = request_body(&settings(), PAGE_SIZE, Some("eyAi"));

        assert_eq!(next["options"]["$skipToken"], "eyAi");
    }

    #[test]
    fn a_page_lists_resources_with_their_group_and_tags() {
        // The shape of Resource Graph's documented "First page query" response.
        let body = json!({
            "totalRecords": 386,
            "count": 2,
            "resultTruncated": "false",
            "facets": [],
            "$skipToken": "eyAibm8iOiAibHVjayJ9",
            "data": [
                {
                    "id": "/subscriptions/cfbbd179-59d2-4052-aa06-9270a38aa9d6/resourceGroups/RG1/providers/Microsoft.Compute/virtualMachines/myVm1",
                    "name": "myVm1",
                    "resourceGroup": "RG1",
                    "tags": { "env": "prod", "cost-center": 42 }
                },
                {
                    "id": "/subscriptions/cfbbd179-59d2-4052-aa06-9270a38aa9d6/resourceGroups/rg2/providers/Microsoft.Compute/virtualMachines/vm2",
                    "tags": null
                }
            ]
        });

        let page = parse_page(&body);

        assert_eq!(page.skip_token.as_deref(), Some("eyAibm8iOiAibHVjayJ9"));
        assert_eq!(page.resources.len(), 2);
        assert_eq!(page.resources[0].name, "myVm1");
        assert_eq!(page.resources[0].resource_group, "RG1");
        assert_eq!(
            page.resources[0].tags,
            [
                ("cost-center".to_string(), "42".to_string()),
                ("env".to_string(), "prod".to_string())
            ]
        );
        assert_eq!(page.resources[1].name, "vm2");
        assert_eq!(page.resources[1].resource_group, "rg2");
        assert!(page.resources[1].tags.is_empty());
    }
}
