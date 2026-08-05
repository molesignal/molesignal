// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Identity-field and IAM-role mapping validation for SSO providers.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    api::AppState,
    domain::iam::{SsoFieldMapping, SsoProviderConfig, SsoProviderKind},
    shared::{Error, Result, ids::Id},
};

pub(super) fn normalize_field_mapping(
    mapping: SsoFieldMapping,
    kind: SsoProviderKind,
) -> Result<SsoFieldMapping> {
    let mapping = SsoFieldMapping {
        subject: mapping.subject.trim().to_owned(),
        email: mapping.email.trim().to_owned(),
        display_name: mapping.display_name.trim().to_owned(),
        groups: mapping.groups.trim().to_owned(),
    };
    for (field, source) in [
        ("subject", mapping.subject.as_str()),
        ("email", mapping.email.as_str()),
        ("display_name", mapping.display_name.as_str()),
        ("groups", mapping.groups.as_str()),
    ] {
        if source.is_empty() || source.len() > 256 || source.chars().any(char::is_control) {
            return Err(Error::invalid(format!(
                "field_mapping.{field} must be 1..256 printable characters"
            )));
        }
        if kind == SsoProviderKind::Oidc && source.split('.').any(|segment| segment.is_empty()) {
            return Err(Error::invalid(format!(
                "field_mapping.{field} must be a valid dotted OIDC claim path"
            )));
        }
        if kind == SsoProviderKind::Ldap && source.chars().any(char::is_whitespace) {
            return Err(Error::invalid(format!(
                "field_mapping.{field} must be an LDAP attribute name"
            )));
        }
    }
    Ok(mapping)
}

pub(super) fn normalize_role_mapping(
    mapping: BTreeMap<String, String>,
    default_role_id: Option<String>,
) -> Result<(BTreeMap<String, String>, Option<String>)> {
    if mapping.len() > 100 {
        return Err(Error::invalid(
            "group_role_mapping must contain at most 100 entries",
        ));
    }
    let mut normalized = BTreeMap::new();
    for (group, role_id) in mapping {
        let group = group.trim();
        let role_id = role_id.trim();
        if group.is_empty() || group.len() > 512 {
            return Err(Error::invalid(
                "group_role_mapping group names must be 1..512 characters",
            ));
        }
        if role_id.is_empty() || role_id.len() > 128 {
            return Err(Error::invalid(
                "group_role_mapping role ids must be 1..128 characters",
            ));
        }
        if normalized
            .insert(group.to_owned(), role_id.to_owned())
            .is_some()
        {
            return Err(Error::invalid(
                "group_role_mapping contains duplicate normalized group names",
            ));
        }
    }
    let default_role_id = default_role_id
        .map(|role_id| role_id.trim().to_owned())
        .filter(|role_id| !role_id.is_empty());
    if default_role_id
        .as_ref()
        .is_some_and(|role_id| role_id.len() > 128)
    {
        return Err(Error::invalid(
            "default_role_id must be at most 128 characters",
        ));
    }
    Ok((normalized, default_role_id))
}

pub(super) async fn ensure_roles_exist(
    state: &AppState,
    org_id: &Id,
    config: &SsoProviderConfig,
) -> Result<()> {
    let referenced = config.referenced_role_ids();
    if referenced.is_empty() {
        return Ok(());
    }
    state.iam.roles.ensure_builtin_roles(org_id).await?;
    let known = state
        .iam
        .roles
        .list(org_id)
        .await?
        .into_iter()
        .filter(|role| role.role_type == "organization" && role.scope == "organization")
        .map(|role| role.id.0)
        .collect::<BTreeSet<_>>();
    let missing = referenced
        .into_iter()
        .filter(|role_id| !known.contains(*role_id))
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(Error::invalid(format!(
            "SSO role mapping references roles outside the target organization: {}",
            missing.join(", ")
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{normalize_field_mapping, normalize_role_mapping};
    use crate::domain::iam::{SsoFieldMapping, SsoProviderKind};

    #[test]
    fn normalizes_group_names_and_rejects_duplicates() {
        let duplicate = BTreeMap::from([
            ("admins".into(), "role-a".into()),
            (" admins ".into(), "role-b".into()),
        ]);
        assert!(normalize_role_mapping(duplicate, None).is_err());
    }

    #[test]
    fn validates_oidc_dotted_claim_paths() {
        let mut mapping = SsoFieldMapping::oidc();
        mapping.groups = "realm_access..roles".into();
        assert!(normalize_field_mapping(mapping, SsoProviderKind::Oidc).is_err());
    }
}
