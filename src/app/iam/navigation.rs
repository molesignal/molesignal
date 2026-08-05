// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Resolve database route policies into browser-ready access decisions.

use std::collections::BTreeSet;

use crate::domain::iam::{
    IamScope,
    navigation::{IamRouteAccess, IamRouteCatalog, IamRoutePermissionMode, IamRouteScope},
};

pub fn resolve_route_access(
    catalog: &IamRouteCatalog,
    scope: IamScope,
    permissions: &BTreeSet<String>,
    features: &BTreeSet<String>,
) -> Vec<IamRouteAccess> {
    catalog
        .routes
        .iter()
        .map(|route| {
            let scope_allowed = match route.scope {
                IamRouteScope::Any => true,
                IamRouteScope::Organization => {
                    matches!(scope, IamScope::Organization | IamScope::ApiToken)
                }
                IamRouteScope::System => scope == IamScope::System,
                IamRouteScope::None => false,
            };
            let features_allowed = route
                .required_features
                .iter()
                .all(|feature| features.contains("*") || features.contains(feature));
            let permissions_allowed = if route.permissions.is_empty() {
                true
            } else {
                match route.permission_mode {
                    IamRoutePermissionMode::All => route
                        .permissions
                        .iter()
                        .all(|permission| permissions.contains(permission)),
                    IamRoutePermissionMode::Any => route
                        .permissions
                        .iter()
                        .any(|permission| permissions.contains(permission)),
                }
            };
            IamRouteAccess {
                id: route.id.clone(),
                path_pattern: route.path_pattern.clone(),
                allowed: route.enabled && scope_allowed && features_allowed && permissions_allowed,
                navigation_group: route.navigation_group.clone(),
                navigation_position: route.navigation_position,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::iam::navigation::{IamRouteDefinition, IamRoutePermissionMode};

    #[test]
    fn route_decisions_apply_scope_features_and_permissions() {
        let catalog = IamRouteCatalog {
            version: 1,
            routes: vec![IamRouteDefinition {
                id: "intelligence".into(),
                path_pattern: "/intelligence".into(),
                scope: IamRouteScope::Organization,
                permission_mode: IamRoutePermissionMode::All,
                permissions: vec!["intelligence.use".into()],
                required_features: vec!["intelligence".into()],
                navigation_group: Some("investigate".into()),
                navigation_position: Some(50),
                enabled: true,
            }],
        };
        let allowed = resolve_route_access(
            &catalog,
            IamScope::Organization,
            &BTreeSet::from(["intelligence.use".into()]),
            &BTreeSet::from(["intelligence".into()]),
        );
        assert!(allowed[0].allowed);
        let denied = resolve_route_access(
            &catalog,
            IamScope::System,
            &BTreeSet::from(["intelligence.use".into()]),
            &BTreeSet::from(["intelligence".into()]),
        );
        assert!(!denied[0].allowed);
    }
}
