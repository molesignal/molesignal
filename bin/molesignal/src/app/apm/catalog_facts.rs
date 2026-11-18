// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What the service catalog says of some services, written for a model: what
//! a service does, who owns it, how much it matters, the other names it goes
//! by and where its data is. A service is found by its name or by one of its
//! other names, case ignored.

use std::fmt::Write as _;

use domain::apm::{ApmCatalogRepository, ServiceAlias, ServiceDefinition, definitions_named};
use kernel::{Result, ids::Id};

/// The most services one text describes.
const MOST_SERVICES: usize = 10;

/// What the catalog says of these services, found by name or alias:
/// description, team, owner, tier, aliases, and where their data is.
/// Services the catalog does not know are left out; `None` when it knows
/// none of them.
pub async fn catalog_facts(
    catalog: &dyn ApmCatalogRepository,
    org_id: &Id,
    services: &[String],
) -> Result<Option<String>> {
    let asked = services
        .iter()
        .map(|service| service.trim())
        .filter(|service| !service.is_empty())
        .collect::<Vec<_>>();

    if asked.is_empty() {
        return Ok(None);
    }

    let definitions = catalog.list_definitions(org_id).await?;

    if definitions.is_empty() {
        return Ok(None);
    }

    // Old names APM reads as a service's own: logs may still carry them.
    let merged = catalog
        .get_settings(org_id)
        .await
        .ok()
        .flatten()
        .map(|settings| settings.aliases)
        .unwrap_or_default();

    let found = found_services(&definitions, &asked);

    if found.is_empty() {
        return Ok(None);
    }

    let mut text = String::from("Service catalog (written by people):");

    for (definition, word) in found {
        text.push('\n');
        text.push_str(&describe(definition, word, &merged));
    }

    Ok(Some(text))
}

/// The definitions the words name, each once, in the order asked: a word
/// that is a service's name finds those services, else those it is another
/// name of. With the word it was found by, when that is not the name.
fn found_services<'a, 'w>(
    definitions: &'a [ServiceDefinition],
    asked: &[&'w str],
) -> Vec<(&'a ServiceDefinition, Option<&'w str>)> {
    let mut found: Vec<(&ServiceDefinition, Option<&str>)> = Vec::new();

    for word in asked {
        for definition in definitions_named(definitions, word) {
            let seen = found.iter().any(|(kept, _)| {
                kept.namespace == definition.namespace && kept.name == definition.name
            });

            if !seen {
                let by_alias = !definition.has_name(word);
                found.push((definition, by_alias.then_some(*word)));
            }
        }
    }

    found.truncate(MOST_SERVICES);
    found
}

/// One service as the catalog keeps it, on a few lines; what is not set is
/// left out. `asked` is the other name it was asked by; `merged`, APM's
/// service aliases of the organization.
fn describe(
    definition: &ServiceDefinition,
    asked: Option<&str>,
    merged: &[ServiceAlias],
) -> String {
    let mut text = format!("- {}/{}", definition.namespace, definition.name);

    if let Some(word) = asked {
        let _ = write!(text, " (asked as \"{word}\")");
    }

    if definition.archived {
        text.push_str(", archived");
    }

    if !definition.description.trim().is_empty() {
        let _ = write!(text, ": {}", definition.description.trim());
    }

    let mut facts = Vec::new();

    if let Some(tier) = definition.tier {
        facts.push(format!("tier {}", tier.as_str()));
    }

    if let Some(lifecycle) = definition.lifecycle {
        facts.push(format!("lifecycle {}", lifecycle.as_str()));
    }

    if let Some(team) = &definition.team_id {
        facts.push(format!("team id {team}"));
    }

    if let Some(owner) = &definition.owner_id {
        facts.push(format!("owner user id {owner}"));
    }

    if let Some(repository) = &definition.repository_url {
        facts.push(format!("repository {repository}"));
    }

    if !facts.is_empty() {
        let _ = write!(text, "\n  {}.", facts.join(", "));
    }

    if !definition.tags.is_empty() {
        let tags = definition
            .tags
            .iter()
            .map(|(key, value)| match value.as_str() {
                "" => key.clone(),
                value => format!("{key}={value}"),
            })
            .collect::<Vec<_>>();

        let _ = write!(text, "\n  Tags: {}.", tags.join(", "));
    }

    if !definition.aliases.is_empty() {
        let _ = write!(
            text,
            "\n  Also known as (in logs, metrics and elsewhere): {}.",
            definition.aliases.join(", ")
        );
    }

    let old_names = merged
        .iter()
        .filter(|alias| alias.namespace == definition.namespace && alias.service == definition.name)
        .map(|alias| format!("{}/{}", alias.alias_namespace, alias.alias))
        .collect::<Vec<_>>();

    if !old_names.is_empty() {
        let _ = write!(
            text,
            "\n  Old names APM reads as this service: {}.",
            old_names.join(", ")
        );
    }

    sources(&mut text, definition);
    text
}

/// Where the service's data is, a line for each kind it lists.
fn sources(text: &mut String, definition: &ServiceDefinition) {
    let sources = &definition.data_sources;

    if !sources.logs.is_empty() {
        let logs = sources
            .logs
            .iter()
            .map(|source| match &source.filter {
                Some(filter) => format!("stream \"{}\" where {filter}", source.stream),
                None => format!("stream \"{}\"", source.stream),
            })
            .collect::<Vec<_>>();

        let _ = write!(text, "\n  Logs: {}.", logs.join("; "));
    }

    if !sources.metrics.is_empty() {
        let metrics = sources
            .metrics
            .iter()
            .map(|source| source.selector.as_str())
            .collect::<Vec<_>>();

        let _ = write!(text, "\n  Metrics: {}.", metrics.join("; "));
    }

    if !sources.dashboards.is_empty() {
        let _ = write!(
            text,
            "\n  Dashboards (ids): {}.",
            sources.dashboards.join(", ")
        );
    }

    if !sources.links.is_empty() {
        let links = sources
            .links
            .iter()
            .map(|link| format!("{} {}", link.name, link.url))
            .collect::<Vec<_>>();

        let _ = write!(text, "\n  Links: {}.", links.join("; "));
    }
}

#[cfg(test)]
mod tests {
    use domain::apm::{
        LogDataSource, MetricDataSource, ServiceDataSources, ServiceLifecycle, ServiceLink,
        ServiceTier,
    };
    use kernel::time::TimestampMicros;

    use super::*;

    fn definition(namespace: &str, name: &str, aliases: &[&str]) -> ServiceDefinition {
        ServiceDefinition {
            aliases: aliases.iter().map(|alias| (*alias).to_owned()).collect(),
            ..ServiceDefinition::blank(namespace, name, Id::from_string("u"), TimestampMicros(0))
        }
    }

    #[test]
    fn services_are_found_by_name_or_other_name_each_once() {
        let kept = vec![
            definition("shop", "order-api", &["order_service", "订单中心"]),
            definition("shop", "billing", &[]),
        ];

        let found = found_services(&kept, &["订单中心", "Order-API", "billing", "unknown"]);
        let names = found
            .iter()
            .map(|(definition, asked)| (definition.name.as_str(), *asked))
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            vec![("order-api", Some("订单中心")), ("billing", None)]
        );
    }

    #[test]
    fn a_description_says_what_is_set_and_where_the_data_is() {
        let full = ServiceDefinition {
            description: "Takes orders.".into(),
            tier: Some(ServiceTier::P0),
            lifecycle: Some(ServiceLifecycle::Production),
            team_id: Some(Id::from_string("team-pay")),
            owner_id: Some(Id::from_string("user-1")),
            tags: [
                ("domain".to_owned(), "orders".to_owned()),
                ("pci".to_owned(), String::new()),
            ]
            .into(),
            data_sources: ServiceDataSources {
                logs: vec![LogDataSource {
                    stream: "default".into(),
                    filter: Some("service_name = 'order_service'".into()),
                }],
                metrics: vec![MetricDataSource {
                    selector: "{service=\"order-api\"}".into(),
                }],
                dashboards: vec!["dash-1".into()],
                links: vec![ServiceLink {
                    name: "Runbook".into(),
                    url: "https://wiki.example.com/orders".into(),
                }],
            },
            ..definition("shop", "order-api", &["order_service"])
        };

        let merged = vec![ServiceAlias {
            alias_namespace: "shop".into(),
            alias: "orders-v1".into(),
            namespace: "shop".into(),
            service: "order-api".into(),
        }];

        let text = describe(&full, Some("order_service"), &merged);

        for expected in [
            "- shop/order-api (asked as \"order_service\"): Takes orders.",
            "tier p0, lifecycle production, team id team-pay, owner user id user-1",
            "Tags: domain=orders, pci.",
            "Also known as (in logs, metrics and elsewhere): order_service.",
            "Old names APM reads as this service: shop/orders-v1.",
            "Logs: stream \"default\" where service_name = 'order_service'.",
            "Metrics: {service=\"order-api\"}.",
            "Dashboards (ids): dash-1.",
            "Links: Runbook https://wiki.example.com/orders.",
        ] {
            assert!(text.contains(expected), "missing {expected:?} in {text}");
        }

        let bare = describe(&definition("shop", "billing", &[]), None, &[]);

        assert_eq!(bare, "- shop/billing");
    }
}
