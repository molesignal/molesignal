// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The service catalog for Mole Agent. `search_service_catalog` finds
//! services by name, other name, description or tag, with what the catalog
//! keeps of each; the other APM tools read a service asked for by one of its
//! other names as the service itself.

use std::collections::HashMap;

use domain::{
    apm::{ServiceAlias, ServiceDefinition},
    iam::IamContext,
};
use kernel::{Error, Result, ids::Id};
use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::ToolResult;

use super::super::{ToolRuntime, common::parse_args};
use crate::app::apm::ApmQueryRequest;

const DEFAULT_LIMIT: usize = 10;
const MAX_LIMIT: usize = 50;

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SearchArgs {
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

pub(super) async fn search(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    // The catalog is read as its page reads it, beside the APM permissions the tool asks for.
    if !auth.permissions.contains("apm.definitions.read")
        && !auth.permissions.contains("sys.telemetry.read")
    {
        return Err(Error::forbidden(
            "reading the service catalog needs apm.definitions.read",
        ));
    }

    let args: SearchArgs = parse_args(arguments)?;
    let query = args.query.as_deref().map(str::trim).unwrap_or_default();
    let limit = args.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let catalog = &runtime.observability.service_catalog;
    let definitions = catalog.list_definitions(&auth.org_id).await?;
    let ranked = rank(&definitions, query);
    let total = ranked.len();
    let shown = ranked.into_iter().take(limit).collect::<Vec<_>>();

    // Old names APM reads as a service's own: logs may still carry them.
    let merged = catalog
        .get_settings(&auth.org_id)
        .await
        .ok()
        .flatten()
        .map(|settings| settings.aliases)
        .unwrap_or_default();

    let people = People::read(runtime, auth, &shown).await;

    let services = shown
        .iter()
        .map(|hit| entry(hit, &merged, &people))
        .collect::<Vec<_>>();

    Ok(ToolResult::json(json!({
        "query": query,
        "total": total,
        "services": services,
    })))
}

/// The service an APM tool was asked for by another name of it.
pub(super) struct AliasResolution {
    pub(super) asked: String,
    pub(super) namespace: String,
    pub(super) service: String,
}

impl AliasResolution {
    /// What the tool's result says of it.
    pub(super) fn note(&self) -> Value {
        json!({
            "asked": self.asked,
            "namespace": self.namespace,
            "service": self.service,
            "reason": "another name of the service in the service catalog",
        })
    }
}

/// The catalog's service `asked` is another name of, when no service is
/// called `asked`: neither one in the catalog nor one APM saw in the range
/// asked about. `None` too when the catalog cannot be read.
pub(super) async fn resolve_alias(
    runtime: &ToolRuntime,
    auth: &IamContext,
    asked: &str,
    request: &ApmQueryRequest,
) -> Result<Option<AliasResolution>> {
    let Ok(definitions) = runtime
        .observability
        .service_catalog
        .list_definitions(&auth.org_id)
        .await
    else {
        return Ok(None);
    };

    let Some(definition) = alias_target(&definitions, asked) else {
        return Ok(None);
    };

    // APM may know a service by that very name without the catalog describing it.
    let probe = ApmQueryRequest {
        service: Some(asked.to_owned()),
        ..request.clone()
    };

    let apm = &runtime.observability.apm;
    let context = apm.context(
        auth.org_id.clone(),
        probe,
        &["request_count"],
        "request_count",
    )?;

    match apm.services(&context).await {
        Ok(page) if page.items.is_empty() => Ok(Some(AliasResolution {
            asked: asked.to_owned(),
            namespace: definition.namespace.clone(),
            service: definition.name.clone(),
        })),
        Ok(_) | Err(_) => Ok(None),
    }
}

/// The service `asked` is another name of, when it is the name of none in
/// the catalog; a service still watched before an archived one.
fn alias_target<'a>(
    definitions: &'a [ServiceDefinition],
    asked: &str,
) -> Option<&'a ServiceDefinition> {
    if definitions
        .iter()
        .any(|definition| definition.has_name(asked))
    {
        return None;
    }

    definitions
        .iter()
        .filter(|definition| definition.has_alias(asked))
        .min_by_key(|definition| definition.archived)
}

/// How a service matched the words asked for, strongest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Matched {
    Name,
    Alias,
    Words,
}

impl Matched {
    fn as_str(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Alias => "alias",
            Self::Words => "words",
        }
    }
}

struct Hit<'a> {
    definition: &'a ServiceDefinition,
    /// `None` when nothing was asked: every service is listed.
    matched: Option<Matched>,
    /// Among word matches, how strongly: words in a name count most.
    weight: usize,
}

/// The services that match `query`, best first: its name, then one of its
/// other names, then every word of it found in a name, another name, a tag
/// or the description. An empty query lists every service, the ones that
/// matter most first. Case is ignored; archived services come after the
/// others that match as well.
fn rank<'a>(definitions: &'a [ServiceDefinition], query: &str) -> Vec<Hit<'a>> {
    let words = query
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>();

    let mut hits = definitions
        .iter()
        .filter_map(|definition| {
            if words.is_empty() {
                return Some(Hit {
                    definition,
                    matched: None,
                    weight: 0,
                });
            }

            if definition.has_name(query) {
                return Some(Hit {
                    definition,
                    matched: Some(Matched::Name),
                    weight: 0,
                });
            }

            if definition.has_alias(query) {
                return Some(Hit {
                    definition,
                    matched: Some(Matched::Alias),
                    weight: 0,
                });
            }

            word_weight(definition, &words).map(|weight| Hit {
                definition,
                matched: Some(Matched::Words),
                weight,
            })
        })
        .collect::<Vec<_>>();

    hits.sort_by(|left, right| {
        left.matched
            .cmp(&right.matched)
            .then_with(|| right.weight.cmp(&left.weight))
            .then_with(|| left.definition.archived.cmp(&right.definition.archived))
            .then_with(|| tier_rank(left.definition).cmp(&tier_rank(right.definition)))
            .then_with(|| left.definition.namespace.cmp(&right.definition.namespace))
            .then_with(|| left.definition.name.cmp(&right.definition.name))
    });

    hits
}

/// How strongly every word is found in the service, or `None` when one is
/// not: a word in its name or another name counts 3, in a tag 2, in its
/// description 1.
fn word_weight(definition: &ServiceDefinition, words: &[String]) -> Option<usize> {
    let names = std::iter::once(&definition.name)
        .chain(&definition.aliases)
        .map(|name| name.to_lowercase())
        .collect::<Vec<_>>();

    let tags = definition
        .tags
        .iter()
        .map(|(key, value)| format!("{key}={value}").to_lowercase())
        .collect::<Vec<_>>();

    let description = definition.description.to_lowercase();
    let mut weight = 0;

    for word in words {
        weight += if names.iter().any(|name| name.contains(word.as_str())) {
            3
        } else if tags.iter().any(|tag| tag.contains(word.as_str())) {
            2
        } else if description.contains(word.as_str()) {
            1
        } else {
            return None;
        };
    }

    Some(weight)
}

/// P0 first, a service without a tier last.
fn tier_rank(definition: &ServiceDefinition) -> u8 {
    definition.tier.map_or(u8::MAX, |tier| tier as u8)
}

/// The names of the teams and owners of the services listed, where they can
/// be read; ids alone otherwise.
#[derive(Default)]
struct People {
    teams: HashMap<String, String>,
    users: HashMap<String, String>,
}

impl People {
    async fn read(runtime: &ToolRuntime, auth: &IamContext, hits: &[Hit<'_>]) -> Self {
        let mut people = Self::default();

        if hits.iter().any(|hit| hit.definition.team_id.is_some())
            && let Ok(teams) = runtime.administration.teams.list(&auth.org_id).await
        {
            people.teams = teams
                .into_iter()
                .map(|team| (team.id.0, team.name))
                .collect();
        }

        let mut owners = hits
            .iter()
            .filter_map(|hit| hit.definition.owner_id.clone())
            .collect::<Vec<Id>>();

        owners.sort_by(|left, right| left.0.cmp(&right.0));
        owners.dedup();

        if !owners.is_empty()
            && let Ok(users) = runtime.administration.iam.users.get_many(&owners).await
        {
            people.users = users
                .into_iter()
                .map(|user| (user.id.0, user.display_name))
                .collect();
        }

        people
    }
}

fn entry(hit: &Hit<'_>, merged: &[ServiceAlias], people: &People) -> Value {
    let definition = hit.definition;

    let merged_names = merged
        .iter()
        .filter(|alias| alias.namespace == definition.namespace && alias.service == definition.name)
        .map(|alias| format!("{}/{}", alias.alias_namespace, alias.alias))
        .collect::<Vec<_>>();

    json!({
        "namespace": definition.namespace,
        "name": definition.name,
        "matchedBy": hit.matched.map(Matched::as_str),
        "description": definition.description,
        "team": definition.team_id.as_ref().map(|id| json!({
            "id": id,
            "name": people.teams.get(&id.0),
        })),
        "owner": definition.owner_id.as_ref().map(|id| json!({
            "id": id,
            "name": people.users.get(&id.0),
        })),
        "tier": definition.tier,
        "lifecycle": definition.lifecycle,
        "repositoryUrl": definition.repository_url,
        "tags": definition.tags,
        "aliases": definition.aliases,
        "mergedNames": merged_names,
        "dataSources": definition.data_sources,
        "archived": definition.archived,
    })
}

#[cfg(test)]
mod tests {
    use domain::apm::ServiceTier;
    use kernel::time::TimestampMicros;

    use super::*;

    fn definition(name: &str, aliases: &[&str], description: &str) -> ServiceDefinition {
        ServiceDefinition {
            aliases: aliases.iter().map(|alias| (*alias).to_owned()).collect(),
            description: description.to_owned(),
            ..ServiceDefinition::blank("shop", name, Id::from_string("u"), TimestampMicros(0))
        }
    }

    fn names(hits: &[Hit<'_>]) -> Vec<String> {
        hits.iter().map(|hit| hit.definition.name.clone()).collect()
    }

    #[test]
    fn a_name_comes_before_another_name_and_both_before_words() {
        let kept = vec![
            definition("orders-report", &[], "Reports on orders."),
            definition("order-api", &["orders"], "Takes orders."),
            definition("orders", &[], "The old order service."),
        ];

        let hits = rank(&kept, "Orders");

        assert_eq!(names(&hits), ["orders", "order-api", "orders-report"]);
        assert_eq!(hits[0].matched, Some(Matched::Name));
        assert_eq!(hits[1].matched, Some(Matched::Alias));
        assert_eq!(hits[2].matched, Some(Matched::Words));
    }

    #[test]
    fn every_word_must_be_found_and_names_weigh_more_than_descriptions() {
        let mut tagged = definition("billing", &[], "");
        tagged.tags.insert("domain".into(), "payments".into());

        let kept = vec![
            definition("ledger", &[], "Keeps payments for billing."),
            tagged,
            definition("checkout", &["订单中心"], "Takes orders and payments."),
        ];

        assert_eq!(
            names(&rank(&kept, "billing payments")),
            ["billing", "ledger"]
        );
        assert_eq!(names(&rank(&kept, "订单")), ["checkout"]);
        assert!(rank(&kept, "shipping").is_empty());
    }

    #[test]
    fn nothing_asked_lists_every_service_that_matters_most_first() {
        let mut archived = definition("legacy", &[], "");
        archived.archived = true;
        archived.tier = Some(ServiceTier::P0);

        let mut critical = definition("payments", &[], "");
        critical.tier = Some(ServiceTier::P0);

        let kept = vec![definition("analytics", &[], ""), archived, critical];

        assert_eq!(
            names(&rank(&kept, "  ")),
            ["payments", "analytics", "legacy"]
        );
    }

    #[test]
    fn another_name_leads_to_its_service_only_when_no_service_has_that_name() {
        let mut retired = definition("orders-old", &["orders"], "");
        retired.archived = true;

        let kept = vec![
            retired,
            definition("order-api", &["orders", "订单中心"], ""),
            definition("billing", &[], ""),
        ];

        assert_eq!(
            alias_target(&kept, "订单中心").map(|definition| definition.name.as_str()),
            Some("order-api")
        );
        assert_eq!(
            alias_target(&kept, "ORDERS").map(|definition| definition.name.as_str()),
            Some("order-api")
        );
        assert!(alias_target(&kept, "billing").is_none());
        assert!(alias_target(&kept, "unknown").is_none());
    }
}
