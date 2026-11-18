// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What a service is called elsewhere and where its data is.
//!
//! The other names logs, metrics, tickets and people use for a service
//! (`order_service`, "Order Center") beside the name APM knows it by, and the
//! log streams, metric selectors, dashboards and links that lead to its data.
//! People write both in the catalog; people and Mole Agent read them to find
//! the service's data under the names it goes by. Neither changes what is
//! collected or how spans are read: APM settings' service aliases do that.

use kernel::{Error, Result};
use serde::{Deserialize, Serialize};

use super::ServiceDefinition;

/// The most other names a service keeps, and the longest one.
pub const CATALOG_ALIASES_MAX: usize = 20;
pub const CATALOG_ALIAS_MAX_LENGTH: usize = 128;

/// The most entries of each kind of data source a service keeps.
pub const DATA_SOURCES_PER_KIND_MAX: usize = 10;

/// The longest log filter and the longest metric selector.
pub const DATA_SOURCE_FILTER_MAX_LENGTH: usize = 2_000;
pub const DATA_SOURCE_SELECTOR_MAX_LENGTH: usize = 1_000;

/// The longest name and address of a link.
pub const DATA_SOURCE_LINK_NAME_MAX_LENGTH: usize = 120;
pub const DATA_SOURCE_LINK_URL_MAX_LENGTH: usize = 2_048;

/// The longest stream name, as streams are named.
const STREAM_NAME_MAX_LENGTH: usize = 255;

/// The longest dashboard id kept.
const DASHBOARD_ID_MAX_LENGTH: usize = 64;

/// Where a service's data is: the log streams that hold its logs, the
/// selectors of its metrics, the dashboards that show it, and pages about it
/// elsewhere. Every list may be empty.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ServiceDataSources {
    pub logs: Vec<LogDataSource>,
    pub metrics: Vec<MetricDataSource>,
    /// Ids of the dashboards that show the service.
    pub dashboards: Vec<String>,
    /// Runbooks, documents, a CMDB entry, the on-call page.
    pub links: Vec<ServiceLink>,
}

/// A log stream that holds the service's logs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogDataSource {
    pub stream: String,
    /// A boolean SQL expression that picks the service's logs out of the
    /// stream; none when the stream holds this service's logs alone.
    #[serde(default)]
    pub filter: Option<String>,
}

/// The service's metrics, as a PromQL selector: `{service="order-api"}`, or
/// a metric with its labels such as `http_server_requests_total{app="orders"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricDataSource {
    pub selector: String,
}

/// A page about the service elsewhere.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceLink {
    pub name: String,
    pub url: String,
}

impl ServiceDataSources {
    pub fn is_empty(&self) -> bool {
        self.logs.is_empty()
            && self.metrics.is_empty()
            && self.dashboards.is_empty()
            && self.links.is_empty()
    }

    /// The same, each text without the spaces around it, entries left empty
    /// dropped and a filter left empty taken as none.
    pub fn trimmed(self) -> Self {
        Self {
            logs: self
                .logs
                .into_iter()
                .map(|source| LogDataSource {
                    stream: source.stream.trim().to_owned(),
                    filter: source
                        .filter
                        .map(|filter| filter.trim().to_owned())
                        .filter(|filter| !filter.is_empty()),
                })
                .filter(|source| !source.stream.is_empty() || source.filter.is_some())
                .collect(),
            metrics: self
                .metrics
                .into_iter()
                .map(|source| MetricDataSource {
                    selector: source.selector.trim().to_owned(),
                })
                .filter(|source| !source.selector.is_empty())
                .collect(),
            dashboards: self
                .dashboards
                .into_iter()
                .map(|id| id.trim().to_owned())
                .filter(|id| !id.is_empty())
                .collect(),
            links: self
                .links
                .into_iter()
                .map(|link| ServiceLink {
                    name: link.name.trim().to_owned(),
                    url: link.url.trim().to_owned(),
                })
                .filter(|link| !link.name.is_empty() || !link.url.is_empty())
                .collect(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        for (kind, count) in [
            ("log streams", self.logs.len()),
            ("metric selectors", self.metrics.len()),
            ("dashboards", self.dashboards.len()),
            ("links", self.links.len()),
        ] {
            if count > DATA_SOURCES_PER_KIND_MAX {
                return Err(Error::invalid(format!(
                    "a service lists at most {DATA_SOURCES_PER_KIND_MAX} {kind}"
                )));
            }
        }

        for source in &self.logs {
            validate_log_source(source)?;
        }

        for source in &self.metrics {
            validate_selector(&source.selector)?;
        }

        for (index, id) in self.dashboards.iter().enumerate() {
            if id.is_empty() || id.len() > DASHBOARD_ID_MAX_LENGTH || !plain(id) {
                return Err(Error::invalid(format!("{id:?} is not a dashboard id")));
            }

            if self.dashboards[..index].contains(id) {
                return Err(Error::invalid(format!(
                    "the dashboard {id} is listed twice"
                )));
            }
        }

        for link in &self.links {
            validate_link(link)?;
        }

        Ok(())
    }
}

fn validate_log_source(source: &LogDataSource) -> Result<()> {
    let stream = source.stream.as_str();

    if stream.is_empty() || stream.len() > STREAM_NAME_MAX_LENGTH || !plain(stream) {
        return Err(Error::invalid(
            "a log source names its stream in 1 to 255 bytes",
        ));
    }

    if let Some(filter) = &source.filter
        && (filter.chars().count() > DATA_SOURCE_FILTER_MAX_LENGTH || !text(filter))
    {
        return Err(Error::invalid(format!(
            "the filter of log stream {stream} is at most {DATA_SOURCE_FILTER_MAX_LENGTH} characters of text"
        )));
    }

    Ok(())
}

/// A selector is a metric name, a `{…}` of labels, or a name followed by its
/// labels: one pair of braces at most, closing it last.
fn validate_selector(selector: &str) -> Result<()> {
    let opened = selector.matches('{').count();
    let closed = selector.matches('}').count();
    let shaped = !selector.is_empty()
        && selector.chars().count() <= DATA_SOURCE_SELECTOR_MAX_LENGTH
        && text(selector)
        && !selector.contains('\n')
        && opened == closed
        && opened <= 1
        && (opened == 0 || selector.ends_with('}'));

    if shaped {
        Ok(())
    } else {
        Err(Error::invalid(format!(
            "{selector:?} is not a PromQL selector such as {{service=\"checkout\"}} or http_requests_total{{app=\"checkout\"}}"
        )))
    }
}

fn validate_link(link: &ServiceLink) -> Result<()> {
    if link.name.is_empty()
        || link.name.chars().count() > DATA_SOURCE_LINK_NAME_MAX_LENGTH
        || !plain(&link.name)
    {
        return Err(Error::invalid(format!(
            "a link is named in 1 to {DATA_SOURCE_LINK_NAME_MAX_LENGTH} characters"
        )));
    }

    let rest = link
        .url
        .strip_prefix("https://")
        .or_else(|| link.url.strip_prefix("http://"));

    let addressed = rest.is_some_and(|rest| {
        !rest.is_empty() && !rest.starts_with('/') && !rest.chars().any(char::is_whitespace)
    }) && link.url.len() <= DATA_SOURCE_LINK_URL_MAX_LENGTH
        && plain(&link.url);

    if addressed {
        Ok(())
    } else {
        Err(Error::invalid(format!(
            "the link {} is an http or https address without spaces",
            link.name
        )))
    }
}

/// Checks the other names of the service `name`: at most
/// [`CATALOG_ALIASES_MAX`], each 1 to [`CATALOG_ALIAS_MAX_LENGTH`] characters
/// of text without spaces around it, never the service's own name, none
/// twice. Case is ignored in the comparisons.
pub(super) fn validate_aliases(name: &str, aliases: &[String]) -> Result<()> {
    if aliases.len() > CATALOG_ALIASES_MAX {
        return Err(Error::invalid(format!(
            "a service has at most {CATALOG_ALIASES_MAX} other names"
        )));
    }

    let own = folded(name);

    for (index, alias) in aliases.iter().enumerate() {
        if alias.is_empty()
            || alias.chars().count() > CATALOG_ALIAS_MAX_LENGTH
            || alias.trim() != alias
            || !plain(alias)
        {
            return Err(Error::invalid(format!(
                "another name of a service is 1 to {CATALOG_ALIAS_MAX_LENGTH} characters of text, without spaces around it: {alias:?}"
            )));
        }

        let word = folded(alias);

        if word == own {
            return Err(Error::invalid(format!(
                "{alias} is the service's own name, not another name of it"
            )));
        }

        if aliases[..index]
            .iter()
            .any(|earlier| folded(earlier) == word)
        {
            return Err(Error::invalid(format!(
                "{alias} is given twice as another name"
            )));
        }
    }

    Ok(())
}

impl ServiceDefinition {
    /// Whether `word` is one of the service's other names, case ignored.
    pub fn has_alias(&self, word: &str) -> bool {
        let word = folded(word.trim());
        self.aliases.iter().any(|alias| folded(alias) == word)
    }

    /// Whether `word` is the service's name, case ignored.
    pub fn has_name(&self, word: &str) -> bool {
        folded(&self.name) == folded(word.trim())
    }
}

/// The definitions `word` names, case ignored: those it is the name of
/// first, then those it is another name of.
pub fn definitions_named<'a>(
    definitions: &'a [ServiceDefinition],
    word: &str,
) -> Vec<&'a ServiceDefinition> {
    let named = definitions
        .iter()
        .filter(|definition| definition.has_name(word));

    let aliased = definitions
        .iter()
        .filter(|definition| !definition.has_name(word) && definition.has_alias(word));

    named.chain(aliased).collect()
}

/// The first of `definition`'s other names that another service of the
/// organization is known by already, its name or one of its other names, as
/// a sentence for whoever saves it. The definition itself, by namespace and
/// name, is not another service.
pub fn alias_clash(definition: &ServiceDefinition, kept: &[ServiceDefinition]) -> Option<String> {
    let others = kept
        .iter()
        .filter(|other| other.namespace != definition.namespace || other.name != definition.name);

    for other in others {
        for alias in &definition.aliases {
            if other.has_name(alias) {
                return Some(format!(
                    "{alias} is the name of the service {}/{}",
                    other.namespace, other.name
                ));
            }

            if other.has_alias(alias) {
                return Some(format!(
                    "{alias} is already another name of the service {}/{}",
                    other.namespace, other.name
                ));
            }
        }
    }

    None
}

/// A word as names are compared: case ignored.
fn folded(word: &str) -> String {
    word.to_lowercase()
}

/// One line of text: no control characters.
fn plain(value: &str) -> bool {
    !value.chars().any(char::is_control)
}

/// Text that may run over lines: no control characters but line breaks and tabs.
fn text(value: &str) -> bool {
    !value
        .chars()
        .any(|char| char.is_control() && !matches!(char, '\n' | '\r' | '\t'))
}

#[cfg(test)]
mod tests {
    use kernel::{ids::Id, time::TimestampMicros};

    use super::*;

    fn definition(namespace: &str, name: &str, aliases: &[&str]) -> ServiceDefinition {
        ServiceDefinition {
            aliases: aliases.iter().map(|alias| (*alias).to_owned()).collect(),
            ..ServiceDefinition::blank(namespace, name, Id::from_string("u"), TimestampMicros(0))
        }
    }

    #[test]
    fn other_names_are_bounded_and_never_the_service_itself_or_twice() {
        let ok = definition("shop", "order-api", &["order_service", "订单中心"]);

        assert!(ok.validate().is_ok());
        assert!(
            definition("shop", "order-api", &["Order-API"])
                .validate()
                .is_err()
        );
        assert!(
            definition("shop", "order-api", &["orders", "ORDERS"])
                .validate()
                .is_err()
        );
        assert!(
            definition("shop", "order-api", &[" orders"])
                .validate()
                .is_err()
        );
        assert!(definition("shop", "order-api", &[""]).validate().is_err());

        let long = "x".repeat(CATALOG_ALIAS_MAX_LENGTH + 1);

        assert!(
            definition("shop", "order-api", &[long.as_str()])
                .validate()
                .is_err()
        );

        let many = (0..=CATALOG_ALIASES_MAX)
            .map(|index| format!("name-{index}"))
            .collect::<Vec<_>>();

        let crowded = ServiceDefinition {
            aliases: many,
            ..ok
        };

        assert!(crowded.validate().is_err());
    }

    #[test]
    fn data_sources_are_shaped_and_bounded() {
        let mut sources = ServiceDataSources {
            logs: vec![LogDataSource {
                stream: "default".into(),
                filter: Some("service_name = 'order_service'".into()),
            }],
            metrics: vec![
                MetricDataSource {
                    selector: "{service=\"order-api\"}".into(),
                },
                MetricDataSource {
                    selector: "http_server_requests_total{app=\"orders\"}".into(),
                },
                MetricDataSource {
                    selector: "orders_queue_depth".into(),
                },
            ],
            dashboards: vec!["dash-1".into()],
            links: vec![ServiceLink {
                name: "Runbook".into(),
                url: "https://wiki.example.com/orders".into(),
            }],
        };

        assert!(sources.validate().is_ok());
        assert!(!sources.is_empty());
        assert!(ServiceDataSources::default().is_empty());

        sources.metrics[0].selector = "{service=\"order-api\"".into();
        assert!(sources.validate().is_err());

        sources.metrics[0].selector = "a{b=\"c\"}{d=\"e\"}".into();
        assert!(sources.validate().is_err());

        sources.metrics[0].selector = "{service=\"order-api\"}".into();
        sources.links[0].url = "ftp://wiki.example.com".into();
        assert!(sources.validate().is_err());

        sources.links[0].url = "https://".into();
        assert!(sources.validate().is_err());

        sources.links[0].url = "https://wiki.example.com/orders".into();
        sources.dashboards.push("dash-1".into());
        assert!(sources.validate().is_err());

        sources.dashboards.pop();
        sources.logs[0].stream = String::new();
        assert!(sources.validate().is_err());

        sources.logs[0].stream = "default".into();
        sources.logs = vec![sources.logs[0].clone(); DATA_SOURCES_PER_KIND_MAX + 1];
        assert!(sources.validate().is_err());
    }

    #[test]
    fn trimming_drops_empty_entries_and_empty_filters() {
        let sources = ServiceDataSources {
            logs: vec![
                LogDataSource {
                    stream: " default ".into(),
                    filter: Some("  ".into()),
                },
                LogDataSource {
                    stream: " ".into(),
                    filter: None,
                },
            ],
            metrics: vec![MetricDataSource {
                selector: " ".into(),
            }],
            dashboards: vec![" dash-1 ".into(), String::new()],
            links: vec![ServiceLink {
                name: " ".into(),
                url: " ".into(),
            }],
        }
        .trimmed();

        assert_eq!(
            sources.logs,
            vec![LogDataSource {
                stream: "default".into(),
                filter: None,
            }]
        );
        assert!(sources.metrics.is_empty());
        assert_eq!(sources.dashboards, vec!["dash-1".to_owned()]);
        assert!(sources.links.is_empty());
    }

    #[test]
    fn a_word_names_services_by_name_first_then_by_other_name() {
        let kept = vec![
            definition("shop", "orders", &[]),
            definition("shop", "order-api", &["Orders-Legacy", "订单中心"]),
            definition("legacy", "orders-legacy", &[]),
        ];

        let found = definitions_named(&kept, "ORDERS-legacy")
            .into_iter()
            .map(|definition| format!("{}/{}", definition.namespace, definition.name))
            .collect::<Vec<_>>();

        assert_eq!(found, vec!["legacy/orders-legacy", "shop/order-api"]);

        let chinese = definitions_named(&kept, "订单中心");

        assert_eq!(chinese.len(), 1);
        assert_eq!(chinese[0].name, "order-api");
        assert!(definitions_named(&kept, "billing").is_empty());
    }

    #[test]
    fn another_name_may_not_be_what_another_service_is_known_by() {
        let kept = vec![
            definition("shop", "billing", &["invoices"]),
            definition("shop", "order-api", &["order_service"]),
        ];

        let saved = definition("shop", "order-api", &["order_service", "Billing"]);
        let clash = alias_clash(&saved, &kept).expect("billing is another service");

        assert!(clash.contains("shop/billing"), "{clash}");

        let taken = definition("shop", "order-api", &["INVOICES"]);
        let clash = alias_clash(&taken, &kept).expect("invoices is taken");

        assert!(clash.contains("already another name"), "{clash}");

        // Its own names, kept before, are not a clash with itself.
        assert_eq!(
            alias_clash(&definition("shop", "order-api", &["order_service"]), &kept),
            None
        );

        // The same name in another namespace is another service.
        assert!(
            alias_clash(
                &definition("legacy", "order-api", &["order_service"]),
                &kept
            )
            .is_some()
        );
    }
}
