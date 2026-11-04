// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The limit on handing out access: nobody gives what they do not hold, and nobody acts on a member
//! who holds more than they do.
//!
//! The rule names no role. An Admin cannot make someone an Owner only because an Owner holds
//! permissions the Admin lacks; a role that holds all of them can. Root holds every organization
//! permission in every organization, so the rule never stops it.

use crate::{
    api::AppState,
    app::iam::IamContext,
    shared::{Error, Result, ids::Id},
};

/// Refuses to hand out a permission the caller does not hold.
pub(crate) fn require_held<'a>(
    ctx: &IamContext,
    permissions: impl IntoIterator<Item = &'a String>,
) -> Result<()> {
    match permissions
        .into_iter()
        .find(|permission| !ctx.has_permission(permission))
    {
        Some(missing) => Err(Error::forbidden(format!(
            "cannot grant {missing}, which you do not hold"
        ))),
        None => Ok(()),
    }
}

/// Refuses to hand out a role that holds a permission the caller does not.
pub(crate) async fn require_roles_held(
    state: &AppState,
    ctx: &IamContext,
    role_ids: &[Id],
) -> Result<()> {
    for role_id in role_ids {
        let permissions = state
            .iam
            .access
            .repository()
            .role_permissions(&ctx.org_id, role_id)
            .await?;

        require_held(ctx, &permissions)?;
    }

    Ok(())
}

/// Refuses to act on a member who holds a permission the caller does not: changing their roles,
/// removing them, disabling them or resetting their second factor would otherwise be a way past
/// the limit on what the caller may give.
pub(crate) async fn require_member_within_reach(
    state: &AppState,
    ctx: &IamContext,
    target: &Id,
) -> Result<()> {
    let roles = state
        .iam
        .service
        .iam_memberships
        .assigned_roles(target, &ctx.org_id)
        .await?;

    for role in roles {
        let permissions = state
            .iam
            .access
            .repository()
            .role_permissions(&ctx.org_id, &role.id)
            .await?;

        if let Some(missing) = permissions
            .iter()
            .find(|permission| !ctx.has_permission(permission))
        {
            return Err(Error::forbidden(format!(
                "this member holds {missing}, which you do not"
            )));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::domain::iam::IamScope;

    fn holding(permissions: &[&str]) -> IamContext {
        IamContext {
            user_id: Id::from_string("admin"),
            org_id: Id::from_string("acme"),
            display_role: "Admin".into(),
            roles: Vec::new(),
            credential_token_id: None,
            credential_application_id: None,
            credential_service_account_id: None,
            scope: IamScope::Organization,
            permissions: permissions
                .iter()
                .map(|key| (*key).to_owned())
                .collect::<BTreeSet<_>>(),
            policy_version: 1,
        }
    }

    #[test]
    fn what_the_caller_holds_can_be_handed_out() {
        let ctx = holding(&["alerts.rules.read", "alerts.rules.manage"]);
        let wanted = ["alerts.rules.read".to_owned()];

        assert!(require_held(&ctx, &wanted).is_ok());
        assert!(require_held(&ctx, &[]).is_ok());
    }

    #[test]
    fn what_the_caller_lacks_cannot_and_the_refusal_names_it() {
        let ctx = holding(&["alerts.rules.read"]);
        let wanted = ["alerts.rules.read".to_owned(), "org.delete".to_owned()];

        let refused = require_held(&ctx, &wanted).unwrap_err();

        assert!(matches!(refused, Error::Forbidden(_)));
        assert!(refused.to_string().contains("org.delete"));
    }
}
