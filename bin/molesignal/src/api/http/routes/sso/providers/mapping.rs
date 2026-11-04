// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Identity-field, IAM-role mapping, and allowed-group validation for SSO providers.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    api::{AppState, http::routes::iam::grant_limit},
    app::iam::IamContext,
    domain::iam::{SsoFieldMapping, SsoProviderConfig, SsoProviderKind},
    infra::sso::claims::claim_sources,
    shared::{Error, Result, ids::Id},
};

/// 允许登录的组最多几项。
const MAX_ALLOWED_GROUPS: usize = 100;

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

        // OIDC 与 OAuth 2.0 读 JSON 声明：可用 `|` 列出几个点分路径。
        let claim_kind = matches!(kind, SsoProviderKind::Oidc | SsoProviderKind::Oauth2);

        if claim_kind
            && claim_sources(source).any(|path| path.split('.').any(|segment| segment.is_empty()))
        {
            return Err(Error::invalid(format!(
                "field_mapping.{field} must list valid dotted claim paths separated by |"
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
            "groupRoleMapping must contain at most 100 entries",
        ));
    }

    let mut normalized = BTreeMap::new();

    for (group, role_id) in mapping {
        let group = group.trim();
        let role_id = role_id.trim();

        if group.is_empty() || group.len() > 512 {
            return Err(Error::invalid(
                "groupRoleMapping group names must be 1..512 characters",
            ));
        }

        if role_id.is_empty() || role_id.len() > 128 {
            return Err(Error::invalid(
                "groupRoleMapping role ids must be 1..128 characters",
            ));
        }

        if normalized
            .insert(group.to_owned(), role_id.to_owned())
            .is_some()
        {
            return Err(Error::invalid(
                "groupRoleMapping contains duplicate normalized group names",
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
            "defaultRoleId must be at most 128 characters",
        ));
    }

    Ok((normalized, default_role_id))
}

/// 去掉空白和重复；组名规则与角色映射相同。
pub(super) fn normalize_allowed_groups(groups: Vec<String>) -> Result<Vec<String>> {
    let mut normalized = Vec::new();

    for group in groups {
        let group = group.trim();

        if group.is_empty() || normalized.iter().any(|known| known == group) {
            continue;
        }

        if group.len() > 512 {
            return Err(Error::invalid(
                "allowedGroups entries must be at most 512 characters",
            ));
        }

        normalized.push(group.to_owned());
    }

    if normalized.len() > MAX_ALLOWED_GROUPS {
        return Err(Error::invalid(format!(
            "allowed_groups must contain at most {MAX_ALLOWED_GROUPS} entries"
        )));
    }

    Ok(normalized)
}

/// A provider hands its roles to whoever signs in through it, so whoever configures it can name
/// only roles they could grant themselves. A role it already named may stay.
pub(super) async fn ensure_roles_grantable(
    state: &AppState,
    ctx: &IamContext,
    config: &SsoProviderConfig,
    previous: Option<&SsoProviderConfig>,
) -> Result<()> {
    let already = previous
        .map(SsoProviderConfig::referenced_role_ids)
        .unwrap_or_default();

    let added = config
        .referenced_role_ids()
        .difference(&already)
        .map(|role_id| Id::from_string(*role_id))
        .collect::<Vec<_>>();

    grant_limit::require_roles_held(state, ctx, &added).await
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

    use super::{normalize_allowed_groups, normalize_field_mapping, normalize_role_mapping};
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

    #[test]
    fn claim_alternatives_are_checked_one_by_one() {
        let mut mapping = SsoFieldMapping::oauth2();
        mapping.email = "data.enterprise_email | data.email".into();
        assert!(normalize_field_mapping(mapping.clone(), SsoProviderKind::Oauth2).is_ok());

        mapping.email = "email | ".into();
        assert!(normalize_field_mapping(mapping, SsoProviderKind::Oauth2).is_err());
    }

    #[test]
    fn allowed_groups_are_trimmed_and_deduplicated() {
        let groups = normalize_allowed_groups(vec![
            " acme ".into(),
            "acme".into(),
            String::new(),
            "acme/sre".into(),
        ])
        .expect("groups");
        assert_eq!(groups, ["acme", "acme/sre"]);
        assert!(normalize_allowed_groups(vec!["g".repeat(513)]).is_err());
    }
}
