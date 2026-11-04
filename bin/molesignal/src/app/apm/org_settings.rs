// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The names an organization reads its services and environments by: its
//! naming rules and environment mapping, put on each fact as spans are read
//! before anything is told by the service's name, and its aliases, read
//! wherever APM reads. The book of every organization's is read again every
//! [`REFRESH_EVERY`], and at once on the node that saves them.

use std::{collections::HashMap, sync::Arc, time::Duration};

use parking_lot::RwLock;
use regex::{Regex, RegexBuilder};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::{
    domain::apm::{
        ApmCatalogRepository, ApmOrgSettings, ApmSpanFact, NAMING_PATTERN_SIZE_LIMIT, NamingRule,
    },
    shared::ids::Id,
};

/// How long settings saved on another node take at most to apply here.
const REFRESH_EVERY: Duration = Duration::from_secs(30);

/// Naming rules compiled, in their order.
#[derive(Debug, Default)]
pub struct ServiceNaming {
    rules: Vec<(Regex, String)>,
}

impl ServiceNaming {
    pub fn compile(rules: &[NamingRule]) -> Self {
        Self {
            rules: rules
                .iter()
                .filter_map(|rule| {
                    RegexBuilder::new(&rule.pattern)
                        .size_limit(NAMING_PATTERN_SIZE_LIMIT)
                        .build()
                        .ok()
                        .map(|pattern| (pattern, rule.replacement.clone()))
                })
                .collect(),
        }
    }

    /// The name `name` is read as: the first rule whose pattern matches
    /// rewrites it; a rewrite to nothing keeps the name.
    pub fn rename(&self, name: &str) -> Option<String> {
        let (pattern, replacement) = self
            .rules
            .iter()
            .find(|(pattern, _)| pattern.is_match(name))?;
        let renamed = pattern
            .replace(name, replacement.as_str())
            .trim()
            .to_owned();

        (!renamed.is_empty() && renamed != name).then_some(renamed)
    }
}

/// An organization's naming rules, environment mapping and aliases, compiled.
#[derive(Default)]
pub struct OrgNames {
    naming: ServiceNaming,
    /// Environments by their lowercased name, to the name they are read as.
    environments: HashMap<String, String>,
    /// Services by an alias of theirs, namespaces aside.
    aliases: HashMap<String, String>,
}

impl OrgNames {
    pub fn of(settings: &ApmOrgSettings) -> Self {
        Self {
            naming: ServiceNaming::compile(&settings.naming_rules),
            environments: settings
                .environment_map
                .iter()
                .map(|mapping| {
                    (
                        mapping.from.trim().to_lowercase(),
                        mapping.to.trim().to_owned(),
                    )
                })
                .collect(),
            aliases: settings
                .aliases
                .iter()
                .map(|alias| (alias.alias.clone(), alias.service.clone()))
                .collect(),
        }
    }

    /// Nothing is read under another name.
    pub fn is_plain(&self) -> bool {
        self.naming.rules.is_empty() && self.environments.is_empty() && self.aliases.is_empty()
    }

    pub fn maps_environments(&self) -> bool {
        !self.environments.is_empty()
    }

    /// The name APM reads a service called `name` in its spans by: renamed by
    /// the naming rules, then an alias read as its service.
    pub fn service(&self, name: &str) -> String {
        let renamed = self.naming.rename(name).unwrap_or_else(|| name.to_owned());

        self.aliases.get(&renamed).cloned().unwrap_or(renamed)
    }

    /// The name APM reads an environment called `environment` in spans by.
    pub fn environment<'a>(&'a self, environment: &'a str) -> &'a str {
        self.environments
            .get(&environment.to_lowercase())
            .map_or(environment, String::as_str)
    }
}

#[derive(Default)]
pub struct OrgSettingsBook {
    orgs: RwLock<HashMap<Id, Arc<OrgNames>>>,
}

impl OrgSettingsBook {
    /// Renames the fact's service and maps its environment as its
    /// organization says. Aliases are left to the reads, so that a service
    /// keeps what it reported under its own name.
    pub fn apply(&self, fact: &mut ApmSpanFact) {
        let Some(names) = self.orgs.read().get(&fact.org_id).cloned() else {
            return;
        };

        if let Some(name) = names.naming.rename(&fact.service.name) {
            fact.service.name = name;
        }

        if let Some(environment) = names
            .environments
            .get(&fact.service.environment.to_lowercase())
        {
            fact.service.environment = environment.clone();
        }
    }

    pub fn replace_all(&self, all: Vec<(Id, ApmOrgSettings)>) {
        *self.orgs.write() = all
            .into_iter()
            .map(|(org_id, settings)| (org_id, Arc::new(OrgNames::of(&settings))))
            .filter(|(_, names)| !names.is_plain())
            .collect();
    }

    pub fn replace_org(&self, org_id: &Id, settings: &ApmOrgSettings) {
        let names = OrgNames::of(settings);
        let mut orgs = self.orgs.write();

        if names.is_plain() {
            orgs.remove(org_id);
        } else {
            orgs.insert(org_id.clone(), Arc::new(names));
        }
    }
}

/// Reads every organization's settings into `book` now and every
/// [`REFRESH_EVERY`] until `cancel`; a failed read keeps what was read before.
pub fn spawn_settings_refresh(
    book: Arc<OrgSettingsBook>,
    catalog: Arc<dyn ApmCatalogRepository>,
    cancel: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(REFRESH_EVERY);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = tick.tick() => match catalog.list_all_settings().await {
                    Ok(all) => book.replace_all(all),
                    Err(error) => tracing::warn!(
                        target: "molesignal::app::apm",
                        error = %error,
                        "APM settings could not be read; keeping the settings read before"
                    ),
                },
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::apm::{EnvironmentMapping, ServiceAlias};

    #[test]
    fn the_first_matching_rule_renames_and_nothing_is_never_a_name() {
        let naming = ServiceNaming::compile(&[
            NamingRule {
                pattern: r"^(?P<base>checkout)-[a-z0-9]{4,}$".into(),
                replacement: "${base}".into(),
            },
            NamingRule {
                pattern: "^checkout".into(),
                replacement: "second".into(),
            },
            NamingRule {
                pattern: "^erase-me$".into(),
                replacement: String::new(),
            },
        ]);

        assert_eq!(naming.rename("checkout-7d9f2").as_deref(), Some("checkout"));
        assert_eq!(naming.rename("checkout").as_deref(), Some("second"));
        assert_eq!(naming.rename("erase-me"), None);
        assert_eq!(naming.rename("cart"), None);
    }

    #[test]
    fn an_organization_maps_its_environments_by_name_ignoring_case() {
        let book = OrgSettingsBook::default();
        let settings = ApmOrgSettings {
            environment_map: vec![EnvironmentMapping {
                from: "Production".into(),
                to: "prod".into(),
            }],
            ..ApmOrgSettings::default()
        };

        book.replace_org(&Id::from_string("org-1"), &settings);

        let names = book
            .orgs
            .read()
            .get(&Id::from_string("org-1"))
            .cloned()
            .expect("compiled");

        assert_eq!(names.environment("PRODUCTION"), "prod");
        assert_eq!(names.environment("staging"), "staging");

        book.replace_org(&Id::from_string("org-1"), &ApmOrgSettings::default());

        assert!(book.orgs.read().is_empty());
    }

    #[test]
    fn a_name_in_spans_is_renamed_then_read_as_the_service_it_is_an_alias_of() {
        let names = OrgNames::of(&ApmOrgSettings {
            naming_rules: vec![NamingRule {
                pattern: r"^(checkout)-[a-z0-9]{5}$".into(),
                replacement: "$1".into(),
            }],
            aliases: vec![ServiceAlias {
                alias_namespace: "shop".into(),
                alias: "checkout".into(),
                namespace: "shop".into(),
                service: "payments".into(),
            }],
            ..ApmOrgSettings::default()
        });

        assert_eq!(names.service("checkout-7d9f2"), "payments");
        assert_eq!(names.service("cart"), "cart");
        assert!(!names.is_plain());
        assert!(OrgNames::default().is_plain());
    }
}
