// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 鉴权 middleware（auth-hardening 双 token 体系）。
//!
//! `Authorization: Bearer <X>` 前缀分发：
//! - `ms_` → API token（`api_tokens` 表 → argon2 verify → IamContext）
//! - 其它 → JWT（`IamService::verify_token`，多 secret 兼容 rotate window）
//!
//! 白名单（`/api/v1/auth/signin` / 密码重置 / `/api/v1/auth/sso/*` /
//! `/api/v1/healthz` /
//! `/metrics` / `/s/*` /
//! `/api/v1/public/share*` /
//! `/api/v1/files/stream/*` / `/api/v1/public/avatars/*`）放行。

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};

use crate::{
    api::AppState,
    infra::persistence::repositories::audit_events::AuditEvent,
    shared::{
        Error,
        ids::Id,
        time::TimestampMicros,
        trace_context::{TraceTrust, update_current_trace_context},
    },
};

mod api_tokens;

use api_tokens::authenticate_bearer_identity;
pub use api_tokens::{authenticate_api_token, authenticate_bearer, verify_api_token};

const WHITELIST_PREFIXES: &[&str] = &[
    "/api/v1/auth/signin",
    "/api/v1/auth/signup", // 自助注册（策略关闭时 handler 自身返 403）
    "/api/v1/auth/forgot-password",
    "/api/v1/auth/reset-password",
    "/api/v1/auth/sso/", // 外部 IdP redirect/callback、公开 provider 列表与 LDAP bind
    "/api/v1/instance",  // 实例公开信息：signin 页读 signup_enabled
    "/api/v1/healthz",
    "/metrics",
    "/s/",                            // 资源分享 token 交换 share session
    "/api/v1/public/share",           // HttpOnly share session 自鉴权
    "/api/v1/files/stream/",          // 文件下载 token 自带授权
    "/api/v1/public/avatars/",        // 头像公开读：<img src> 不带 Bearer
    "/api/v1/billing/stripe/webhook", // Stripe webhook：无 JWT，靠 HMAC 验签
    // Push 型 connector 接入：外部平台带不了 Bearer，由 handler 用 X-Connector-Token 自鉴权。
    "/api/v1/_kinesis_firehose",
    "/api/v1/_cloudflare",
    "/api/v1/_heroku",
];

fn is_whitelisted_path(path: &str) -> bool {
    WHITELIST_PREFIXES
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

pub async fn auth_layer(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, Error> {
    let path = req.uri().path().to_string();
    if is_whitelisted_path(&path) {
        return Ok(next.run(req).await);
    }
    let token = req
        .headers()
        .get(http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .ok_or_else(|| Error::unauthorized("missing Authorization Bearer token"))?;

    // Organization state is enforced by the inner org-blocking layer so a
    // disabled tenant can still reach the narrowly-scoped recovery routes.
    let mut ctx = authenticate_bearer_identity(
        token,
        state.iam.service.as_ref(),
        state.iam.api_tokens.clone(),
    )
    .await?;
    let capability_snapshot = state.iam.access.enrich_context(&mut ctx).await?;
    let debug_token = req
        .headers()
        .get(crate::shared::trace_context::TRACE_DEBUG_TOKEN)
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.starts_with("mstd_") && value.len() <= 160)
        .map(str::to_owned);
    if let Some(debug_token) = debug_token {
        let token_hash = blake3::hash(debug_token.as_bytes()).to_hex().to_string();
        if let Some(grant) = state
            .telemetry
            .trace_debug_tokens
            .consume(
                &token_hash,
                Some(&ctx.org_id),
                Some(&path),
                TimestampMicros::now(),
            )
            .await?
        {
            if let Some(trace_context) = req
                .extensions_mut()
                .get_mut::<crate::shared::trace_context::TraceContext>()
            {
                trace_context.trust = TraceTrust::DebugToken;
                trace_context.apply_trusted_force(true);
            }
            update_current_trace_context(|trace_context| {
                trace_context.trust = TraceTrust::DebugToken;
                trace_context.apply_trusted_force(true);
            });
            let _ = state
                .iam
                .audit_events
                .record(AuditEvent {
                    id: Id::new(),
                    org_id: state.iam.system_org_id.clone(),
                    actor_kind: "user".into(),
                    actor_id: ctx.user_id.0.clone(),
                    action: "trace_debug_token.use".into(),
                    target_kind: Some("trace_debug_token".into()),
                    target_id: Some(grant.id.0),
                    ip: None,
                    user_agent: None,
                    payload: serde_json::json!({
                        "organization_id": ctx.org_id.0,
                        "route": path,
                        "used_count": grant.used_count,
                    }),
                    ts: TimestampMicros::now(),
                })
                .await;
        }
    }
    if let Some(trace_context) = req
        .extensions_mut()
        .get_mut::<crate::shared::trace_context::TraceContext>()
    {
        trace_context.set_authenticated_org(ctx.org_id.as_str());
    }
    update_current_trace_context(|trace_context| {
        trace_context.set_authenticated_org(ctx.org_id.as_str());
    });
    tracing::Span::current().record("molesignal.org.id", ctx.org_id.as_str());
    tracing::Span::current().record("molesignal.user.id", ctx.user_id.as_str());
    req.extensions_mut().insert(capability_snapshot);
    req.extensions_mut().insert(ctx);
    Ok(next.run(req).await)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;

    use super::{authenticate_bearer, is_whitelisted_path};
    use crate::{
        app::iam::IamService,
        config::AuthSettings,
        domain::iam::{
            IamAssignedRole, IamMembership, IamMembershipRepository, Organization,
            OrganizationRepository, User, UserRepository, UserStatus,
            api_token::{ApiToken, ApiTokenKind, ApiTokenRepository, ManagedApiToken},
        },
        infra::persistence::repositories::api_tokens::{
            assemble_rum_token, assemble_token, generate_token_parts, hash_rum_secret, hash_secret,
        },
        shared::{Error, Result, ids::Id, time::TimestampMicros},
    };

    struct TestUsers {
        user: Mutex<User>,
    }

    impl TestUsers {
        fn set_disabled(&self, disabled: bool) {
            self.user.lock().expect("lock test user").disabled = disabled;
        }
    }

    #[async_trait]
    impl UserRepository for TestUsers {
        async fn create(&self, user: User) -> Result<User> {
            *self.user.lock().expect("lock test user") = user.clone();
            Ok(user)
        }

        async fn get(&self, id: &Id) -> Result<User> {
            let user = self.user.lock().expect("lock test user");
            if &user.id == id {
                Ok(user.clone())
            } else {
                Err(Error::not_found("user"))
            }
        }

        async fn get_by_email(&self, email: &str) -> Result<User> {
            let user = self.user.lock().expect("lock test user");
            if user.email == email {
                Ok(user.clone())
            } else {
                Err(Error::not_found("user"))
            }
        }

        async fn update(&self, user: User) -> Result<User> {
            *self.user.lock().expect("lock test user") = user.clone();
            Ok(user)
        }

        async fn delete(&self, _id: &Id) -> Result<()> {
            Ok(())
        }

        async fn count(&self) -> Result<u64> {
            Ok(1)
        }

        async fn list(&self) -> Result<Vec<User>> {
            Ok(vec![self.user.lock().expect("lock test user").clone()])
        }

        async fn set_status(&self, _id: &Id, status: UserStatus) -> Result<()> {
            self.user.lock().expect("lock test user").status = status;
            Ok(())
        }
    }

    struct TestOrganizations {
        organization: Mutex<Organization>,
    }

    impl TestOrganizations {
        fn set_disabled(&self, disabled: bool) {
            self.organization
                .lock()
                .expect("lock test organization")
                .disabled = disabled;
        }
    }

    #[async_trait]
    impl OrganizationRepository for TestOrganizations {
        async fn create(&self, org: Organization) -> Result<Organization> {
            *self.organization.lock().expect("lock test organization") = org.clone();
            Ok(org)
        }

        async fn get(&self, id: &Id) -> Result<Organization> {
            let organization = self.organization.lock().expect("lock test organization");
            if &organization.id == id {
                Ok(organization.clone())
            } else {
                Err(Error::not_found("organization"))
            }
        }

        async fn get_by_slug(&self, slug: &str) -> Result<Organization> {
            let organization = self.organization.lock().expect("lock test organization");
            if organization.slug == slug {
                Ok(organization.clone())
            } else {
                Err(Error::not_found("organization"))
            }
        }

        async fn list(&self) -> Result<Vec<Organization>> {
            Ok(vec![
                self.organization
                    .lock()
                    .expect("lock test organization")
                    .clone(),
            ])
        }

        async fn update_name(&self, id: &Id, name: String) -> Result<Organization> {
            let mut organization = self.organization.lock().expect("lock test organization");
            if &organization.id != id {
                return Err(Error::not_found("organization"));
            }
            organization.name = name;
            Ok(organization.clone())
        }

        async fn set_disabled(&self, id: &Id, disabled: bool) -> Result<Organization> {
            let mut organization = self.organization.lock().expect("lock test organization");
            if &organization.id != id {
                return Err(Error::not_found("organization"));
            }
            organization.disabled = disabled;
            Ok(organization.clone())
        }

        async fn delete(&self, _id: &Id) -> Result<()> {
            Ok(())
        }
    }

    struct UnusedMemberships;

    #[async_trait]
    impl IamMembershipRepository for UnusedMemberships {
        async fn upsert(
            &self,
            _membership: IamMembership,
            _role_ids: &[Id],
            _actor_id: &Id,
        ) -> Result<()> {
            Ok(())
        }

        async fn list_for_user(&self, _user_id: &Id) -> Result<Vec<IamMembership>> {
            Ok(Vec::new())
        }

        async fn list_for_org(&self, _org_id: &Id) -> Result<Vec<IamMembership>> {
            Ok(Vec::new())
        }

        async fn assigned_roles(
            &self,
            _user_id: &Id,
            _org_id: &Id,
        ) -> Result<Vec<IamAssignedRole>> {
            Ok(Vec::new())
        }

        async fn role_id_for_purpose(&self, _org_id: &Id, _purpose: &str) -> Result<Id> {
            Err(Error::internal("unused test membership repository"))
        }

        async fn remove(&self, _user_id: &Id, _org_id: &Id) -> Result<()> {
            Ok(())
        }
    }

    struct TestApiTokens {
        token: ApiToken,
    }

    #[async_trait]
    impl ApiTokenRepository for TestApiTokens {
        async fn create(&self, token: ApiToken) -> Result<ApiToken> {
            Ok(token)
        }

        async fn find_by_prefix(&self, prefix: &str) -> Result<Option<ApiToken>> {
            Ok((self.token.prefix == prefix).then(|| self.token.clone()))
        }

        async fn list_by_org(&self, _org_id: &Id) -> Result<Vec<ApiToken>> {
            Ok(vec![self.token.clone()])
        }

        async fn get(&self, _org_id: &Id, _id: &Id) -> Result<ApiToken> {
            Ok(self.token.clone())
        }

        async fn mark_revoked(&self, _org_id: &Id, _id: &Id) -> Result<()> {
            Ok(())
        }

        async fn touch_last_used(&self, _prefix: &str, _at: TimestampMicros) -> Result<()> {
            Ok(())
        }

        async fn ensure_default(
            &self,
            _org_id: &Id,
            _user_id: &Id,
            _role_id: &Id,
        ) -> Result<ManagedApiToken> {
            Err(Error::internal("unused test API token repository"))
        }

        async fn ensure_rum_client(
            &self,
            _org_id: &Id,
            _user_id: &Id,
            _role_id: &Id,
            _application_id: &str,
        ) -> Result<ManagedApiToken> {
            Err(Error::internal("unused test API token repository"))
        }
    }

    #[test]
    fn promql_capabilities_requires_authentication() {
        assert!(!is_whitelisted_path("/api/v1/query/promql/capabilities"));
        assert!(is_whitelisted_path("/api/v1/healthz"));
        assert!(is_whitelisted_path("/api/v1/auth/sso/providers"));
        assert!(is_whitelisted_path("/api/v1/auth/sso/ldap/login"));
        assert!(!is_whitelisted_path("/api/v1/sso/providers"));
    }

    #[tokio::test]
    async fn disabled_user_or_organization_invalidates_existing_credentials() {
        let user_id = Id::from_string("user-1");
        let org_id = Id::from_string("org-1");
        let users = Arc::new(TestUsers {
            user: Mutex::new(User {
                id: user_id.clone(),
                email: "user@test.example".into(),
                display_name: "User".into(),
                avatar_url: None,
                bio: String::new(),
                password_hash: String::new(),
                disabled: false,
                status: UserStatus::Active,
                created_at: TimestampMicros::now(),
            }),
        });
        let organizations = Arc::new(TestOrganizations {
            organization: Mutex::new(Organization {
                id: org_id.clone(),
                name: "Test organization".into(),
                slug: "test-organization".into(),
                system: false,
                disabled: false,
                created_at: TimestampMicros::now(),
            }),
        });
        let iam = IamService::new(
            users.clone(),
            organizations.clone(),
            Arc::new(UnusedMemberships),
            AuthSettings {
                deprecated_jwt_secret: None,
                token_ttl_secs: 3_600,
                root_email: String::new(),
                root_password: String::new(),
            },
            vec![b"auth-state-test-secret".to_vec()],
        );
        let jwt = iam.issue_token(&user_id, &org_id).expect("issue test JWT");

        let (prefix, secret) = generate_token_parts();
        let api_token = assemble_token(&prefix, &secret);
        let api_tokens = Arc::new(TestApiTokens {
            token: ApiToken {
                id: Id::from_string("token-1"),
                prefix,
                secret_hash: hash_secret(&secret).expect("hash test API token"),
                org_id: org_id.clone(),
                user_id: user_id.clone(),
                role_id: Id::from_string("role-1"),
                name: "test".into(),
                expires_at: None,
                last_used_at: None,
                revoked: false,
                created_at: TimestampMicros::now(),
                is_default: false,
                token_kind: ApiTokenKind::Personal,
                application_id: None,
            },
        });

        assert!(
            authenticate_bearer(&jwt, &iam, api_tokens.clone())
                .await
                .is_ok()
        );
        assert!(
            authenticate_bearer(&api_token, &iam, api_tokens.clone())
                .await
                .is_ok()
        );

        let (rum_prefix, rum_secret) = generate_token_parts();
        let rum_token = assemble_rum_token(&rum_prefix, &rum_secret);
        let rum_tokens = Arc::new(TestApiTokens {
            token: ApiToken {
                id: Id::from_string("rum-token-1"),
                prefix: rum_prefix,
                secret_hash: hash_rum_secret(&rum_secret),
                org_id: org_id.clone(),
                user_id: user_id.clone(),
                role_id: Id::from_string("rum-role-1"),
                name: "rum".into(),
                expires_at: None,
                last_used_at: None,
                revoked: false,
                created_at: TimestampMicros::now(),
                is_default: false,
                token_kind: ApiTokenKind::RumClient,
                application_id: Some("mobile-shop".into()),
            },
        });
        assert!(
            authenticate_bearer(&rum_token, &iam, rum_tokens.clone())
                .await
                .is_ok()
        );

        users.set_disabled(true);

        for credential in [&jwt, &api_token] {
            assert!(matches!(
                authenticate_bearer(credential, &iam, api_tokens.clone()).await,
                Err(Error::Unauthorized(message)) if message == "user disabled"
            ));
        }
        assert!(
            authenticate_bearer(&rum_token, &iam, rum_tokens.clone())
                .await
                .is_ok(),
            "an application credential must not inherit issuer suspension"
        );

        users.set_disabled(false);
        organizations.set_disabled(true);
        for credential in [&jwt, &api_token] {
            assert!(matches!(
                authenticate_bearer(credential, &iam, api_tokens.clone()).await,
                Err(Error::Forbidden(message))
                    if message == "organization is disabled by a platform administrator"
            ));
        }
        assert!(matches!(
            authenticate_bearer(&rum_token, &iam, rum_tokens).await,
            Err(Error::Forbidden(message))
                if message == "organization is disabled by a platform administrator"
        ));
    }
}
