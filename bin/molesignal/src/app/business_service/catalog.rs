// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The catalog of business services and what each depends on. A dependency
//! must name something that exists, and business services cannot depend on
//! each other in a circle.

use std::collections::{HashMap, HashSet};

use super::BusinessServiceService;
use crate::{
    domain::{
        business_service::{
            BusinessService, BusinessServiceDependency, BusinessServiceHealth, DependencyKind,
        },
        status_page::StatusPageComponent,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const MAX_NAME_CHARS: usize = 120;
const MAX_DESCRIPTION_CHARS: usize = 500;
const MAX_DEPENDENCIES: usize = 100;
const MAX_TARGET_CHARS: usize = 255;
const MAX_SERVICES_PER_ORG: usize = 1_000;

#[derive(Debug, Clone)]
pub struct BusinessServiceInput {
    pub name: String,
    pub description: String,
    pub dependencies: Vec<BusinessServiceDependency>,
}

impl BusinessServiceService {
    pub async fn list(&self, org_id: &Id) -> Result<Vec<BusinessService>> {
        self.repository.list_business_services(org_id).await
    }

    pub async fn get(&self, org_id: &Id, id: &Id) -> Result<BusinessService> {
        self.repository.get_business_service(org_id, id).await
    }

    /// The status-page components that stand for the business service, or
    /// for any business service with `None`.
    pub async fn linked_components(
        &self,
        org_id: &Id,
        id: Option<&Id>,
    ) -> Result<Vec<StatusPageComponent>> {
        self.status_pages
            .list_service_linked_components(org_id, id)
            .await
    }

    pub async fn create(
        &self,
        org_id: &Id,
        actor_id: &Id,
        input: BusinessServiceInput,
    ) -> Result<BusinessService> {
        let existing = self.repository.list_business_services(org_id).await?;

        if existing.len() >= MAX_SERVICES_PER_ORG {
            return Err(Error::invalid(
                "an organization can have at most 1000 business services",
            ));
        }

        let id = Id::new();
        let input = self.validate(org_id, &id, input, &existing).await?;
        let now = TimestampMicros::now();

        let created = self
            .repository
            .create_business_service(BusinessService {
                id,
                org_id: org_id.clone(),
                name: input.name,
                description: input.description,
                dependencies: input.dependencies,
                health: BusinessServiceHealth::unevaluated(),
                created_by: Some(actor_id.clone()),
                created_at: now,
                updated_at: now,
            })
            .await?;

        self.signals.request(org_id);
        Ok(created)
    }

    pub async fn update(
        &self,
        org_id: &Id,
        id: &Id,
        input: BusinessServiceInput,
    ) -> Result<BusinessService> {
        let existing = self.repository.list_business_services(org_id).await?;

        let current = existing
            .iter()
            .find(|service| &service.id == id)
            .cloned()
            .ok_or_else(|| Error::not_found("business service not found"))?;

        let input = self.validate(org_id, id, input, &existing).await?;

        let updated = self
            .repository
            .update_business_service(BusinessService {
                name: input.name,
                description: input.description,
                dependencies: input.dependencies,
                updated_at: TimestampMicros::now(),
                ..current
            })
            .await?;

        self.signals.request(org_id);
        Ok(updated)
    }

    pub async fn delete(&self, org_id: &Id, id: &Id) -> Result<()> {
        self.repository
            .delete_business_service(org_id, id, TimestampMicros::now())
            .await?;

        // Services that depended on it lose that dependency.
        self.signals.request(org_id);
        Ok(())
    }

    async fn validate(
        &self,
        org_id: &Id,
        id: &Id,
        input: BusinessServiceInput,
        existing: &[BusinessService],
    ) -> Result<BusinessServiceInput> {
        let name = input.name.trim().to_string();

        if name.is_empty()
            || name.chars().count() > MAX_NAME_CHARS
            || name.chars().any(char::is_control)
        {
            return Err(Error::invalid(
                "a business service name must contain 1 to 120 characters",
            ));
        }

        let description = input.description.trim().to_string();

        if description.chars().count() > MAX_DESCRIPTION_CHARS {
            return Err(Error::invalid(
                "a business service description cannot exceed 500 characters",
            ));
        }

        let dependencies = normalize_dependencies(input.dependencies)?;
        self.check_targets_exist(org_id, &dependencies, existing)
            .await?;
        check_no_cycle(id, &dependencies, existing)?;

        Ok(BusinessServiceInput {
            name,
            description,
            dependencies,
        })
    }

    async fn check_targets_exist(
        &self,
        org_id: &Id,
        dependencies: &[BusinessServiceDependency],
        existing: &[BusinessService],
    ) -> Result<()> {
        let named = |kind| {
            dependencies
                .iter()
                .any(|dependency| dependency.kind == kind)
        };

        let monitors: HashSet<String> = if named(DependencyKind::SyntheticMonitor) {
            self.synthetics
                .list_monitors(org_id)
                .await?
                .into_iter()
                .map(|monitor| monitor.id.0)
                .collect()
        } else {
            HashSet::new()
        };

        let rules: HashSet<String> = if named(DependencyKind::AlertRule) {
            self.alert_rules
                .list(org_id)
                .await?
                .into_iter()
                .map(|rule| rule.id.0)
                .collect()
        } else {
            HashSet::new()
        };

        for dependency in dependencies {
            let exists = match dependency.kind {
                DependencyKind::TechnicalService => true,
                DependencyKind::SyntheticMonitor => monitors.contains(&dependency.target),
                DependencyKind::AlertRule => rules.contains(&dependency.target),
                DependencyKind::BusinessService => existing
                    .iter()
                    .any(|service| service.id.as_str() == dependency.target),
            };

            if !exists {
                return Err(Error::not_found(format!(
                    "{} {} not found",
                    dependency.kind.as_str().replace('_', " "),
                    dependency.target
                )));
            }
        }

        Ok(())
    }
}

fn normalize_dependencies(
    dependencies: Vec<BusinessServiceDependency>,
) -> Result<Vec<BusinessServiceDependency>> {
    if dependencies.len() > MAX_DEPENDENCIES {
        return Err(Error::invalid(
            "a business service can have at most 100 dependencies",
        ));
    }

    let mut seen = HashSet::new();
    let mut normalized = Vec::with_capacity(dependencies.len());

    for mut dependency in dependencies {
        dependency.target = dependency.target.trim().to_string();

        if dependency.target.is_empty()
            || dependency.target.chars().count() > MAX_TARGET_CHARS
            || dependency.target.chars().any(char::is_control)
        {
            return Err(Error::invalid(
                "a dependency must name its target in 1 to 255 characters",
            ));
        }

        let key = match dependency.kind {
            DependencyKind::TechnicalService => dependency.target.to_lowercase(),
            _ => dependency.target.clone(),
        };

        if !seen.insert((dependency.kind, key)) {
            return Err(Error::invalid(format!(
                "{} is named twice",
                dependency.target
            )));
        }

        normalized.push(dependency);
    }

    Ok(normalized)
}

/// Refuses dependencies through which the service would depend on itself.
fn check_no_cycle(
    id: &Id,
    dependencies: &[BusinessServiceDependency],
    existing: &[BusinessService],
) -> Result<()> {
    let mut graph: HashMap<&str, Vec<&str>> = existing
        .iter()
        .filter(|service| &service.id != id)
        .map(|service| (service.id.as_str(), business_targets(&service.dependencies)))
        .collect();

    graph.insert(id.as_str(), business_targets(dependencies));

    let mut stack: Vec<&str> = graph.get(id.as_str()).cloned().unwrap_or_default();
    let mut visited = HashSet::new();

    while let Some(next) = stack.pop() {
        if next == id.as_str() {
            return Err(Error::invalid(
                "business services cannot depend on each other in a circle",
            ));
        }

        if visited.insert(next) {
            stack.extend(graph.get(next).cloned().unwrap_or_default());
        }
    }

    Ok(())
}

fn business_targets(dependencies: &[BusinessServiceDependency]) -> Vec<&str> {
    dependencies
        .iter()
        .filter(|dependency| dependency.kind == DependencyKind::BusinessService)
        .map(|dependency| dependency.target.as_str())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::business_service::DependencyImpact;

    fn on(kind: DependencyKind, target: &str) -> BusinessServiceDependency {
        BusinessServiceDependency {
            kind,
            target: target.into(),
            impact: DependencyImpact::Outage,
        }
    }

    fn service(id: &str, dependencies: Vec<BusinessServiceDependency>) -> BusinessService {
        BusinessService {
            id: Id(id.into()),
            org_id: Id("org".into()),
            name: id.into(),
            description: String::new(),
            dependencies,
            health: BusinessServiceHealth::unevaluated(),
            created_by: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    #[test]
    fn a_dependency_is_named_once_and_technical_names_ignore_case() {
        let twice = vec![
            on(DependencyKind::TechnicalService, "payments-api"),
            on(DependencyKind::TechnicalService, " Payments-API "),
        ];
        assert!(normalize_dependencies(twice).is_err());

        let normalized =
            normalize_dependencies(vec![on(DependencyKind::AlertRule, "  rule-1 ")]).unwrap();
        assert_eq!(normalized[0].target, "rule-1");
        assert!(normalize_dependencies(vec![on(DependencyKind::AlertRule, "  ")]).is_err());
    }

    #[test]
    fn a_circle_through_other_services_is_refused() {
        let existing = [
            service(
                "checkout",
                vec![on(DependencyKind::BusinessService, "payments")],
            ),
            service(
                "payments",
                vec![on(DependencyKind::BusinessService, "ledger")],
            ),
            service("ledger", Vec::new()),
        ];

        let circle = [on(DependencyKind::BusinessService, "checkout")];
        assert!(check_no_cycle(&Id("ledger".into()), &circle, &existing).is_err());

        let fine = [on(DependencyKind::BusinessService, "ledger")];
        assert!(check_no_cycle(&Id("checkout".into()), &fine, &existing).is_ok());
    }
}
