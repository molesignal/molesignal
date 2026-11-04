// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A member as the organization finds them now, for work done in their name without a request of
//! theirs: the checks a request goes through (account, organization), then their roles and
//! permissions.

use super::{IamAccessService, IamContext, IamService};
use crate::{
    domain::iam::IamScope,
    shared::{Error, Result, ids::Id},
};

pub struct Member {
    pub email: String,
    pub context: IamContext,
}

/// The member `user_id` of `org_id`. When they cannot act there, the error is what `unusable` makes
/// of who they are and why not; any other error is IAM failing, nothing to do with them.
pub async fn member_context(
    iam: &IamService,
    access: &IamAccessService,
    org_id: &Id,
    user_id: &Id,
    unusable: impl Fn(&str, &str) -> Error,
) -> Result<Member> {
    let user = match iam.current_user(user_id).await {
        Ok(user) => user,
        Err(Error::NotFound(_)) => {
            return Err(unusable(&format!("user {}", user_id.0), "no longer exists"));
        }
        Err(error) => return Err(error),
    };

    let admitted = async {
        iam.ensure_user_access(user_id).await?;
        iam.ensure_organization_access(org_id).await
    };

    if let Err(error) = admitted.await {
        return Err(match error {
            Error::Unauthorized(reason) | Error::Forbidden(reason) => unusable(
                &user.email,
                &format!("can no longer sign in to this organization ({reason})"),
            ),
            other => other,
        });
    }

    let mut context = IamContext {
        user_id: user_id.clone(),
        org_id: org_id.clone(),
        display_role: String::new(),
        roles: Vec::new(),
        credential_token_id: None,
        credential_application_id: None,
        credential_service_account_id: None,
        scope: IamScope::Organization,
        permissions: Default::default(),
        policy_version: 0,
    };

    if let Err(error) = access.enrich_context(&mut context).await {
        return Err(match error {
            Error::Unauthorized(reason) | Error::Forbidden(reason) => unusable(
                &user.email,
                &format!("is no longer a member of this organization ({reason})"),
            ),
            other => other,
        });
    }

    Ok(Member {
        email: user.email,
        context,
    })
}
