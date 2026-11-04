// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What an organization sets for APM: the lines that make a service a
//! warning or critical, the Apdex threshold T of a service without one of its
//! own, from what P99 an endpoint is a long task, how service names and
//! environments are rewritten as spans are read, which services are another
//! service under an older name, and what the catalog asks of a service by its
//! tier.

use std::collections::BTreeSet;

use regex::RegexBuilder;
use serde::{Deserialize, Serialize};

use super::{DEFAULT_SERVICE_NAMESPACE, ServiceTier};
use crate::shared::{Error, Result};

pub const NAMING_RULES_MAX: usize = 20;
pub const ENVIRONMENT_MAPPINGS_MAX: usize = 50;
pub const SERVICE_ALIASES_MAX: usize = 100;
/// The most a compiled naming pattern may take, in bytes.
pub const NAMING_PATTERN_SIZE_LIMIT: usize = 64 * 1024;
const MINUTE_MICROS: u64 = 60_000_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ApmOrgSettings {
    pub health: HealthLines,
    /// The Apdex threshold T of a service whose catalog entry sets none.
    pub apdex_threshold: u64,
    /// From what P99 an endpoint is a long task.
    pub long_task_time: u64,
    /// Tried in order as a span is read; the first whose pattern matches the
    /// service's name rewrites it.
    pub naming_rules: Vec<NamingRule>,
    pub environment_map: Vec<EnvironmentMapping>,
    pub aliases: Vec<ServiceAlias>,
    pub metadata_rules: MetadataRules,
}

impl Default for ApmOrgSettings {
    fn default() -> Self {
        Self {
            health: HealthLines::default(),
            apdex_threshold: 500_000,
            long_task_time: 1_000_000,
            naming_rules: Vec::new(),
            environment_map: Vec::new(),
            aliases: Vec::new(),
            metadata_rules: MetadataRules::default(),
        }
    }
}

/// From what error rate and P95 a service is a warning, and critical.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HealthLines {
    pub error_rate_warning: f64,
    pub error_rate_critical: f64,
    pub p95_warning_time: u64,
    pub p95_critical_time: u64,
}

impl Default for HealthLines {
    fn default() -> Self {
        Self {
            error_rate_warning: 0.02,
            error_rate_critical: 0.10,
            p95_warning_time: 500_000,
            p95_critical_time: 2_000_000,
        }
    }
}

/// A service name matching `pattern` (a regular expression) becomes
/// `replacement`, which may name its groups (`$1`, `${name}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamingRule {
    pub pattern: String,
    pub replacement: String,
}

/// An environment named `from` is read as `to`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentMapping {
    pub from: String,
    pub to: String,
}

/// The service `alias_namespace/alias` is `namespace/service` under another
/// name: what it reported is read as the other's. A namespace left out is
/// the default one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAlias {
    #[serde(default = "default_namespace")]
    pub alias_namespace: String,
    pub alias: String,
    #[serde(default = "default_namespace")]
    pub namespace: String,
    pub service: String,
}

fn default_namespace() -> String {
    DEFAULT_SERVICE_NAMESPACE.to_owned()
}

/// What the catalog asks of a service from a tier on: a team, an owner, a
/// repository. `None` asks nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MetadataRules {
    pub team_from_tier: Option<ServiceTier>,
    pub owner_from_tier: Option<ServiceTier>,
    pub repository_from_tier: Option<ServiceTier>,
}

impl ApmOrgSettings {
    /// Trims the names people typed; a namespace left blank is the default one.
    pub fn normalize(&mut self) {
        for mapping in &mut self.environment_map {
            mapping.from = mapping.from.trim().to_owned();
            mapping.to = mapping.to.trim().to_owned();
        }

        for alias in &mut self.aliases {
            for namespace in [&mut alias.alias_namespace, &mut alias.namespace] {
                *namespace = namespace.trim().to_owned();

                if namespace.is_empty() {
                    *namespace = default_namespace();
                }
            }

            alias.alias = alias.alias.trim().to_owned();
            alias.service = alias.service.trim().to_owned();
        }
    }

    pub fn validate(&self) -> Result<()> {
        self.health.validate()?;

        if self.apdex_threshold == 0 || self.apdex_threshold > MINUTE_MICROS {
            return Err(Error::invalid(
                "the default Apdex threshold is above 0 and at most a minute",
            ));
        }

        if !(1_000..=10 * MINUTE_MICROS).contains(&self.long_task_time) {
            return Err(Error::invalid("a long task is from 1 ms to 10 minutes"));
        }

        validate_naming_rules(&self.naming_rules)?;
        validate_environment_map(&self.environment_map)?;
        validate_aliases(&self.aliases)
    }
}

impl HealthLines {
    fn validate(&self) -> Result<()> {
        let rates = 0.0 < self.error_rate_warning
            && self.error_rate_warning < self.error_rate_critical
            && self.error_rate_critical <= 1.0;
        let latencies = 0 < self.p95_warning_time
            && self.p95_warning_time < self.p95_critical_time
            && self.p95_critical_time <= 10 * MINUTE_MICROS;

        if !rates || !latencies {
            return Err(Error::invalid(
                "a warning line is above 0 and below its critical line; error rates are at most 1, P95 at most 10 minutes",
            ));
        }

        Ok(())
    }
}

fn validate_naming_rules(rules: &[NamingRule]) -> Result<()> {
    if rules.len() > NAMING_RULES_MAX {
        return Err(Error::invalid(format!(
            "at most {NAMING_RULES_MAX} naming rules"
        )));
    }

    for rule in rules {
        if rule.pattern.trim().is_empty() || rule.pattern.chars().count() > 256 {
            return Err(Error::invalid(
                "a naming pattern has from 1 to 256 characters",
            ));
        }

        if rule.replacement.chars().count() > 128 {
            return Err(Error::invalid(
                "a naming replacement has at most 128 characters",
            ));
        }

        RegexBuilder::new(&rule.pattern)
            .size_limit(NAMING_PATTERN_SIZE_LIMIT)
            .build()
            .map_err(|error| Error::invalid(format!("naming pattern {}: {error}", rule.pattern)))?;
    }

    Ok(())
}

fn validate_environment_map(map: &[EnvironmentMapping]) -> Result<()> {
    if map.len() > ENVIRONMENT_MAPPINGS_MAX {
        return Err(Error::invalid(format!(
            "at most {ENVIRONMENT_MAPPINGS_MAX} environment mappings"
        )));
    }

    let mut seen = BTreeSet::new();

    for mapping in map {
        let bounded = |value: &str| !value.trim().is_empty() && value.chars().count() <= 128;

        if !bounded(&mapping.from) || !bounded(&mapping.to) {
            return Err(Error::invalid(
                "an environment mapping names both environments, each in at most 128 characters",
            ));
        }

        if !seen.insert(mapping.from.trim().to_lowercase()) {
            return Err(Error::invalid(format!(
                "the environment {} is mapped twice",
                mapping.from
            )));
        }
    }

    Ok(())
}

fn validate_aliases(aliases: &[ServiceAlias]) -> Result<()> {
    if aliases.len() > SERVICE_ALIASES_MAX {
        return Err(Error::invalid(format!(
            "at most {SERVICE_ALIASES_MAX} service aliases"
        )));
    }

    let names = aliases
        .iter()
        .map(|alias| (alias.alias_namespace.as_str(), alias.alias.as_str()))
        .collect::<Vec<_>>();

    for (index, alias) in aliases.iter().enumerate() {
        if alias.alias.trim().is_empty()
            || alias.service.trim().is_empty()
            || alias.alias.chars().count() > 255
            || alias.service.chars().count() > 255
        {
            return Err(Error::invalid(
                "an alias names both services, each in at most 255 characters",
            ));
        }

        if (alias.alias_namespace.as_str(), alias.alias.as_str())
            == (alias.namespace.as_str(), alias.service.as_str())
        {
            return Err(Error::invalid("a service is not its own alias"));
        }

        if names[..index].contains(&names[index]) {
            return Err(Error::invalid(format!("{} is an alias twice", alias.alias)));
        }

        // The service an alias leads to is never another's alias: aliases do not chain.
        if names.contains(&(alias.namespace.as_str(), alias.service.as_str())) {
            return Err(Error::invalid(format!(
                "{} is itself an alias",
                alias.service
            )));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alias(alias: &str, service: &str) -> ServiceAlias {
        ServiceAlias {
            alias_namespace: "shop".into(),
            alias: alias.into(),
            namespace: "shop".into(),
            service: service.into(),
        }
    }

    #[test]
    fn the_defaults_hold_and_the_lines_keep_their_order() {
        assert!(ApmOrgSettings::default().validate().is_ok());

        let inverted = ApmOrgSettings {
            health: HealthLines {
                error_rate_warning: 0.2,
                ..HealthLines::default()
            },
            ..ApmOrgSettings::default()
        };

        assert!(inverted.validate().is_err());
    }

    #[test]
    fn naming_patterns_compile_and_mappings_are_said_once() {
        let rules = |pattern: &str| ApmOrgSettings {
            naming_rules: vec![NamingRule {
                pattern: pattern.into(),
                replacement: "$1".into(),
            }],
            ..ApmOrgSettings::default()
        };

        assert!(rules("^(checkout)-[a-z0-9]+$").validate().is_ok());
        assert!(rules("(unclosed").validate().is_err());

        let twice = ApmOrgSettings {
            environment_map: vec![
                EnvironmentMapping {
                    from: "Production".into(),
                    to: "prod".into(),
                },
                EnvironmentMapping {
                    from: "production".into(),
                    to: "live".into(),
                },
            ],
            ..ApmOrgSettings::default()
        };

        assert!(twice.validate().is_err());
    }

    #[test]
    fn a_blank_namespace_is_the_default_one() {
        let mut settings = ApmOrgSettings {
            aliases: vec![ServiceAlias {
                alias_namespace: " ".into(),
                alias: " checkout-v1 ".into(),
                namespace: String::new(),
                service: "checkout".into(),
            }],
            ..ApmOrgSettings::default()
        };

        settings.normalize();

        assert_eq!(
            settings.aliases[0].alias_namespace,
            DEFAULT_SERVICE_NAMESPACE
        );
        assert_eq!(settings.aliases[0].alias, "checkout-v1");
        assert_eq!(settings.aliases[0].namespace, DEFAULT_SERVICE_NAMESPACE);
    }

    #[test]
    fn aliases_lead_to_a_service_that_is_no_alias() {
        let settings = |aliases| ApmOrgSettings {
            aliases,
            ..ApmOrgSettings::default()
        };

        assert!(
            settings(vec![alias("checkout-v1", "checkout")])
                .validate()
                .is_ok()
        );
        assert!(
            settings(vec![alias("checkout", "checkout")])
                .validate()
                .is_err()
        );
        assert!(
            settings(vec![alias("a", "checkout"), alias("a", "cart")])
                .validate()
                .is_err()
        );
        assert!(
            settings(vec![alias("a", "b"), alias("b", "c")])
                .validate()
                .is_err()
        );
    }
}
