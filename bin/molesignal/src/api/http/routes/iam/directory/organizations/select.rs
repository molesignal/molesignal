// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 切换组织：同一次登录进入另一个组织，会话 Cookie 随之重写。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::post,
};
use serde::{Deserialize, Serialize};

use super::{caller_roles, has_platform_access, is_root};
use crate::{
    api::{
        AppState,
        http::{
            routes::{
                auth,
                mfa::{self, Checked, FactorProof},
                sso::{self, PublicProviderResponse},
            },
            session_cookie::{self, SessionCookies},
        },
    },
    app::iam::{IamContext, SignInSession, signin_policy::Admission},
    domain::iam::{IamAssignedRole, IamScope, Organization, super_administrator},
    infra::persistence::repositories::audit_events::AuditEvent,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/orgs/{id}/select", post(select_org))
}

#[derive(Debug, Default, Deserialize)]
struct SelectRequest {
    /// 这个组织要求多因素认证时，当场验证的第二因素。
    #[serde(default)]
    proof: Option<FactorProof>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SelectOrgResp {
    user_id: String,
    org_id: String,
    org_name: String,
    display_role: String,
    roles: Vec<IamAssignedRole>,
    system: bool,
}

/// 这次登录要进这个组织还差什么，回答 `{"mfa": …}`、`{"mfaSetup": …}` 或 `{"sso": …}`。
#[derive(Serialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
enum Waiting {
    /// 当场验证一次第二因素，带着 `proof` 再请求。
    Mfa {
        organization_name: String,
        methods: Vec<&'static str>,
    },
    /// 账号还没有第二因素：先到账号页开启，开启后这次登录就算过了。
    MfaSetup { organization_name: String },
    /// 只能经这个组织的单点登录进入。
    Sso {
        organization_name: String,
        providers: Vec<PublicProviderResponse>,
    },
}

/// 这次登录能不能进 `organization`（[`mfa::admission`]）。能进时给出要用的会话：当场验证过的话，
/// 它带着新的确认时间。
async fn passing(
    state: &AppState,
    context: &IamContext,
    session: SignInSession,
    organization: &Organization,
    proof: Option<&FactorProof>,
) -> Result<std::result::Result<SignInSession, Waiting>> {
    let organization_name = organization.name.clone();

    match mfa::admission(state, &organization.id, &session.user_id, &session.method).await? {
        Admission::Allowed => Ok(Ok(session)),
        Admission::NeedsSso => Ok(Err(Waiting::Sso {
            organization_name,
            providers: sso::redirect_providers_of(state, &organization.id).await?,
        })),
        Admission::NeedsMfa => {
            if !mfa::enabled(state, &session.user_id).await? {
                return Ok(Err(Waiting::MfaSetup { organization_name }));
            }

            let Some(proof) = proof else {
                return Ok(Err(Waiting::Mfa {
                    organization_name,
                    methods: mfa::second_step_methods(state, &session.user_id).await?,
                }));
            };

            let user = state.iam.service.users.get(&session.user_id).await?;

            if let Checked::Failed = mfa::check_factor(state, &user, &context.org_id, proof).await?
            {
                return Err(Error::invalid(mfa::rejection(proof)));
            }

            Ok(Ok(state.iam.service.reconfirm_session(&session, true)))
        }
    }
}

/// 同一次登录换到另一个组织：重写两个 Cookie，登录结束的时间不变。目标组织的登录要求不满足时，
/// 回答还差什么（[`Waiting`]）。
async fn select_org(
    State(state): State<AppState>,
    session: Option<Extension<SignInSession>>,
    Extension(context): Extension<IamContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
    request: Option<Json<SelectRequest>>,
) -> Result<Response> {
    let proof = request.and_then(|Json(request)| request.proof);
    let target_org_id = Id::from_string(id);
    let organization = state.iam.service.orgs.get(&target_org_id).await?;
    organization.ensure_enabled()?;

    let root = is_root(&state, &context).await?;

    if organization.system {
        // Hidden from everyone who cannot enter it, as any organization one is not part of.
        if !has_platform_access(&state, &context, root).await? {
            return Err(Error::not_found("organization"));
        }

        let session = browser_session(session)?;

        let session =
            match passing(&state, &context, session, &organization, proof.as_ref()).await? {
                Ok(session) => session,
                Err(waiting) => return Ok(Json(waiting).into_response()),
            };

        return select_system_organization(
            state,
            context,
            (&session, &headers),
            root,
            organization,
        )
        .await
        .map(IntoResponse::into_response);
    }

    let roles = if root {
        vec![super_administrator()]
    } else {
        state
            .iam
            .service
            .iam_memberships
            .list_for_user(&context.user_id)
            .await?
            .into_iter()
            .find(|membership| membership.org_id == target_org_id)
            .ok_or_else(|| Error::forbidden("not a member of this organization"))?;

        state
            .iam
            .service
            .iam_memberships
            .assigned_roles(&context.user_id, &target_org_id)
            .await?
    };

    let display_role = roles
        .iter()
        .map(|role| role.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    let session = browser_session(session)?;

    let session = match passing(&state, &context, session, &organization, proof.as_ref()).await? {
        Ok(session) => session,
        Err(waiting) => return Ok(Json(waiting).into_response()),
    };

    let switched =
        state
            .iam
            .service
            .switch_session(&session, &target_org_id, IamScope::Organization);

    Ok((
        session_cookie::issue(&state, &headers, &switched)?,
        Json(SelectOrgResp {
            user_id: context.user_id.0,
            org_id: target_org_id.0,
            org_name: organization.name,
            display_role,
            roles,
            system: false,
        }),
    )
        .into_response())
}

/// 换组织的是浏览器的登录会话；API token 换不出登录会话。
fn browser_session(session: Option<Extension<SignInSession>>) -> Result<SignInSession> {
    session
        .map(|Extension(session)| session)
        .ok_or_else(|| Error::forbidden("switching organization needs a signed-in browser"))
}

async fn select_system_organization(
    state: AppState,
    context: IamContext,
    (session, headers): (&SignInSession, &HeaderMap),
    root: bool,
    organization: Organization,
) -> Result<(SessionCookies, Json<SelectOrgResp>)> {
    let roles = caller_roles(&state, &context, root, &organization).await?;
    let display_role = auth::display_role(&roles);

    let switched = state
        .iam
        .service
        .switch_session(session, &organization.id, IamScope::System);

    let cookies = session_cookie::issue(&state, headers, &switched)?;

    let _ = state
        .iam
        .audit_events
        .record(AuditEvent {
            id: Id::new(),
            org_id: state.iam.system_org_id.clone(),
            actor_kind: "user".into(),
            actor_id: context.user_id.0.clone(),
            action: "system_scope.issue".into(),
            target_kind: Some("organization".into()),
            target_id: Some(organization.id.0.clone()),
            ip: None,
            user_agent: None,
            payload: serde_json::json!({
                "maxLifetimeSeconds": 3600,
                "displayRole": display_role,
                "roles": roles.iter().map(|role| role.key.as_str()).collect::<Vec<_>>(),
            }),
            ts: TimestampMicros::now(),
        })
        .await;

    Ok((
        cookies,
        Json(SelectOrgResp {
            user_id: context.user_id.0,
            org_id: organization.id.0,
            org_name: organization.name,
            display_role,
            roles,
            system: true,
        }),
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn what_is_missing_is_named_by_the_key_of_the_answer() {
        let mfa = Waiting::Mfa {
            organization_name: "acme".into(),
            methods: vec!["totp", "recovery_code"],
        };

        assert_eq!(
            serde_json::to_value(mfa).unwrap(),
            json!({"mfa": {"organizationName": "acme", "methods": ["totp", "recovery_code"]}})
        );

        let setup = Waiting::MfaSetup {
            organization_name: "acme".into(),
        };

        assert_eq!(
            serde_json::to_value(setup).unwrap(),
            json!({"mfaSetup": {"organizationName": "acme"}})
        );

        let sso = Waiting::Sso {
            organization_name: "acme".into(),
            providers: Vec::new(),
        };

        assert_eq!(
            serde_json::to_value(sso).unwrap(),
            json!({"sso": {"organizationName": "acme", "providers": []}})
        );
    }

    #[test]
    fn a_switch_carries_a_proof_only_when_asked_for_one() {
        let plain: SelectRequest = serde_json::from_value(json!({})).unwrap();
        assert!(plain.proof.is_none());

        let verified: SelectRequest = serde_json::from_value(json!({
            "proof": {"method": "totp", "code": "123456"},
        }))
        .unwrap();
        assert!(matches!(verified.proof, Some(FactorProof::Totp { .. })));
    }
}
