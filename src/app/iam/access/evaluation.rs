// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Resource-level authorization evaluation and bounded IAM conditions.

use std::collections::BTreeSet;

use serde_json::Value;

use super::{
    IamAccessRequest, IamAccessService, IamAttributes, IamDecision, IamDecisionReason, IamTarget,
};
use crate::{
    app::iam::IamContext,
    domain::iam::{
        IamScope,
        access::{IamCrossOrgGrantQuery, IamRoleBinding, ResolvedIamRoleBinding},
        catalog::IamPermissionScope,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

impl IamAccessService {
    pub async fn evaluate(
        &self,
        context: &IamContext,
        request: &IamAccessRequest,
    ) -> Result<IamDecision> {
        let Some(permission_scope) = self
            .repository
            .permission_scope(&request.permission)
            .await?
        else {
            return Err(Error::invalid(format!(
                "unknown permission: {}",
                request.permission
            )));
        };

        if context.scope == IamScope::System {
            if permission_scope != IamPermissionScope::Platform {
                return Ok(IamDecision::deny(
                    IamDecisionReason::ScopeBoundary,
                    context.policy_version,
                ));
            }
            return Ok(if context.has_permission(&request.permission) {
                IamDecision::allow(
                    IamDecisionReason::PlatformAssignment,
                    context.policy_version,
                )
            } else {
                IamDecision::deny(IamDecisionReason::DefaultDeny, context.policy_version)
            });
        }
        if permission_scope != IamPermissionScope::Organization {
            return Ok(IamDecision::deny(
                IamDecisionReason::ScopeBoundary,
                context.policy_version,
            ));
        }

        let target_org = request
            .target
            .as_ref()
            .and_then(|target| target.organization_id.as_deref())
            .map(Id::from_string)
            .unwrap_or_else(|| context.org_id.clone());
        if target_org != context.org_id {
            let Some(target) = request.target.as_ref() else {
                return Ok(IamDecision::deny(
                    IamDecisionReason::TenantIsolation,
                    context.policy_version,
                ));
            };
            let grants = self
                .repository
                .matching_cross_org_grants(&IamCrossOrgGrantQuery {
                    source_organization_id: target_org,
                    target_organization_id: context.org_id.clone(),
                    user_id: context.user_id.clone(),
                    resource_type: target.resource_type.clone(),
                    resource_id: target.resource_id.clone(),
                    permission: request.permission.clone(),
                    now: TimestampMicros::now(),
                })
                .await?;
            let matched = grants
                .into_iter()
                .filter(|grant| conditions_match(&grant.conditions, &request.attributes))
                .map(|grant| grant.id.0)
                .collect::<Vec<_>>();
            if !matched.is_empty() {
                let mut decision = IamDecision::allow(
                    IamDecisionReason::CrossOrganizationGrant,
                    context.policy_version,
                );
                decision.matched_grant_ids = matched;
                return Ok(decision);
            }
            return Ok(IamDecision::deny(
                IamDecisionReason::TenantIsolation,
                context.policy_version,
            ));
        }

        if context.has_permission(&request.permission) {
            return Ok(IamDecision::allow(
                IamDecisionReason::RolePermission,
                context.policy_version,
            ));
        }
        let Some(target) = request.target.as_ref() else {
            return Ok(IamDecision::deny(
                IamDecisionReason::DefaultDeny,
                context.policy_version,
            ));
        };

        let bindings = self
            .repository
            .active_role_bindings(&context.org_id, &context.user_id, TimestampMicros::now())
            .await?;
        let mut condition_mismatch = false;
        let matched_bindings = bindings
            .into_iter()
            .filter(|resolved| {
                if !binding_matches_target(&resolved.binding, target) {
                    return false;
                }
                if !conditions_match(&resolved.binding.conditions, &request.attributes) {
                    condition_mismatch = true;
                    return false;
                }
                validated_binding_permissions(resolved).contains(&request.permission)
            })
            .map(|resolved| resolved.binding.id.0)
            .collect::<Vec<_>>();
        if !matched_bindings.is_empty() {
            let mut decision = IamDecision::allow(
                IamDecisionReason::ResourceRoleBinding,
                context.policy_version,
            );
            decision.matched_binding_ids = matched_bindings;
            return Ok(decision);
        }

        let relationships = self
            .repository
            .matching_relationships(
                &context.org_id,
                &context.user_id,
                &target.resource_type,
                &target.resource_id,
                target.container_type.as_deref(),
                target.container_id.as_deref(),
            )
            .await?;
        let matched_relationships = relationships
            .into_iter()
            .filter(|resolved| {
                resolved
                    .permissions
                    .iter()
                    .any(|permission| permission == &request.permission)
            })
            .map(|resolved| resolved.relationship.id.0)
            .collect::<Vec<_>>();
        if !matched_relationships.is_empty() {
            let mut decision = IamDecision::allow(
                IamDecisionReason::ResourceRelationship,
                context.policy_version,
            );
            decision.matched_relationship_ids = matched_relationships;
            return Ok(decision);
        }

        Ok(IamDecision::deny(
            if condition_mismatch {
                IamDecisionReason::ConditionMismatch
            } else {
                IamDecisionReason::DefaultDeny
            },
            context.policy_version,
        ))
    }

    pub async fn evaluate_batch(
        &self,
        context: &IamContext,
        requests: &[IamAccessRequest],
    ) -> Result<Vec<IamDecision>> {
        if requests.len() > 100 {
            return Err(Error::invalid(
                "IAM access batch cannot contain more than 100 requests",
            ));
        }
        let mut decisions = Vec::with_capacity(requests.len());
        for request in requests {
            decisions.push(self.evaluate(context, request).await?);
        }
        Ok(decisions)
    }
}

fn validated_binding_permissions(binding: &ResolvedIamRoleBinding) -> BTreeSet<String> {
    binding.permissions.iter().cloned().collect()
}

pub(super) fn binding_matches_target(binding: &IamRoleBinding, target: &IamTarget) -> bool {
    let Some(resource_type) = binding.resource_type.as_deref() else {
        return binding.resource_id.is_none();
    };
    if resource_type != target.resource_type {
        return false;
    }
    binding
        .resource_id
        .as_deref()
        .is_none_or(|resource_id| resource_id == target.resource_id)
}

pub(super) fn conditions_match(conditions: &Value, attributes: &IamAttributes) -> bool {
    let Some(object) = conditions.as_object() else {
        return conditions.is_null();
    };
    if object.is_empty() {
        return true;
    }
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "environment" | "labels"))
    {
        return false;
    }
    if let Some(expected) = object.get("environment") {
        let matches = match expected {
            Value::String(value) => attributes.environment.as_deref() == Some(value),
            Value::Array(values) => attributes.environment.as_ref().is_some_and(|actual| {
                values
                    .iter()
                    .any(|value| value.as_str() == Some(actual.as_str()))
            }),
            _ => false,
        };
        if !matches {
            return false;
        }
    }
    if let Some(expected) = object.get("labels") {
        let Some(expected) = expected.as_object() else {
            return false;
        };
        if !expected.iter().all(|(key, value)| {
            value.as_str().is_some_and(|value| {
                attributes
                    .labels
                    .get(key)
                    .is_some_and(|actual| actual == value)
            })
        }) {
            return false;
        }
    }
    true
}

pub fn validate_iam_conditions(conditions: &Value) -> Result<()> {
    let Some(object) = conditions.as_object() else {
        if conditions.is_null() {
            return Ok(());
        }
        return Err(Error::invalid("IAM conditions must be an object"));
    };
    for (key, value) in object {
        match key.as_str() {
            "environment"
                if value.is_string()
                    || value.as_array().is_some_and(|items| {
                        !items.is_empty() && items.iter().all(Value::is_string)
                    }) => {}
            "labels"
                if value
                    .as_object()
                    .is_some_and(|labels| labels.values().all(Value::is_string)) => {}
            "environment" | "labels" => {
                return Err(Error::invalid(format!(
                    "invalid IAM condition value for {key}"
                )));
            }
            _ => {
                return Err(Error::invalid(format!("unsupported IAM condition: {key}")));
            }
        }
    }
    Ok(())
}
