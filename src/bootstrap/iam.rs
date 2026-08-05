// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! IAM、认证、SSO 与安全审计运行时装配。

use std::sync::Arc;

use super::{core::Core, license::LicenseRuntime};
use crate::{
    app::iam::{IamAccessService, IamService},
    config::Settings,
    domain::iam::{
        IamMembership, IamMembershipRepository, IamPlatformAdministratorRepository,
        InstanceSettingsRepository, Organization, OrganizationRepository, SsoProviderRepository,
        api_token::ApiTokenRepository,
    },
    infra::{
        persistence::repositories::{
            api_tokens::PgApiTokenRepository,
            audit_events::{AuditEventRepository, PgAuditEventRepository},
            email_domains::{EmailDomainRepository, PgEmailDomainRepository},
            iam::{
                PgIamRepository,
                memberships::PgIamMembershipRepository,
                roles::{IamRoleRepository, PgIamRoleRepository},
            },
            instance_settings::PgInstanceSettingsRepository,
            invitations::{InvitationRepository, PgInvitationRepository},
            organizations::PgOrganizationRepository,
            signing_secrets::{
                PgSigningSecretRepository, SigningSecretRepository, bootstrap_or_load_jwt_secret,
            },
            sso_providers::PgSsoProviderRepository,
            user_preferences::{PgUserPreferencesRepository, UserPreferencesRepository},
            workspace_preference_defaults::{
                PgWorkspacePreferenceDefaultsRepository, WorkspacePreferenceDefaultsRepository,
            },
        },
        sso::{JwksCache, PgSsoSessionRepository, SsoSessionRepository, SsoStateStore},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) struct IamRuntime {
    pub(super) iam: Arc<IamService>,
    pub(super) access: Arc<IamAccessService>,
    pub(super) instance_settings: Arc<dyn InstanceSettingsRepository>,
    pub(super) signing_secrets: Arc<dyn SigningSecretRepository>,
    pub(super) api_tokens: Arc<dyn ApiTokenRepository>,
    pub(super) user_preferences: Arc<dyn UserPreferencesRepository>,
    pub(super) workspace_preference_defaults: Arc<dyn WorkspacePreferenceDefaultsRepository>,
    pub(super) invitations: Arc<dyn InvitationRepository>,
    pub(super) roles: Arc<dyn IamRoleRepository>,
    pub(super) email_domains: Arc<dyn EmailDomainRepository>,
    pub(super) audit_events: Arc<dyn AuditEventRepository>,
    pub(super) sso_state_store: Arc<SsoStateStore>,
    pub(super) sso_jwks_cache: Arc<JwksCache>,
    pub(super) sso_sessions: Arc<dyn SsoSessionRepository>,
    pub(super) sso_providers: Arc<dyn SsoProviderRepository>,
}

impl IamRuntime {
    pub(super) async fn build(
        settings: &Settings,
        core: &Core,
        license: &LicenseRuntime,
    ) -> Result<Self> {
        let signing_secrets: Arc<dyn SigningSecretRepository> =
            Arc::new(PgSigningSecretRepository::new(core.pool.clone()));
        let jwt_override = std::env::var("MS_AUTH_JWT_SECRET_OVERRIDE").ok();
        let (jwt_active_secrets, jwt_primary_kid) =
            bootstrap_or_load_jwt_secret(signing_secrets.as_ref(), jwt_override.as_deref()).await?;
        tracing::info!(
            kid = %jwt_primary_kid,
            active_count = jwt_active_secrets.len(),
            "JWT signing secrets ready"
        );

        let instance_settings: Arc<dyn InstanceSettingsRepository> =
            Arc::new(PgInstanceSettingsRepository::new(core.pool.clone()));
        let iam = Arc::new(IamService::new(
            core.users.clone(),
            core.orgs.clone(),
            core.iam_memberships.clone(),
            settings.auth.clone(),
            jwt_active_secrets,
        ));
        let api_tokens: Arc<dyn ApiTokenRepository> = Arc::new(
            PgApiTokenRepository::new(core.pool.clone()).with_cipher(core.cipher_root_key.clone()),
        );
        let user_preferences: Arc<dyn UserPreferencesRepository> =
            Arc::new(PgUserPreferencesRepository::new(core.pool.clone()));
        let workspace_preference_defaults: Arc<dyn WorkspacePreferenceDefaultsRepository> =
            Arc::new(PgWorkspacePreferenceDefaultsRepository::new(
                core.pool.clone(),
            ));

        seed_root_if_needed(
            &iam,
            core.orgs.as_ref(),
            core.iam_memberships.as_ref(),
            settings,
        )
        .await?;
        bootstrap_platform_administrator(&iam, core.iam_platform_administrators.as_ref(), settings)
            .await?;

        let invitations: Arc<dyn InvitationRepository> =
            Arc::new(PgInvitationRepository::new(core.pool.clone()));
        let roles: Arc<dyn IamRoleRepository> =
            Arc::new(PgIamRoleRepository::new(core.pool.clone()));
        let iam_repository: Arc<dyn crate::domain::iam::access::IamRepository> =
            Arc::new(PgIamRepository::new(core.pool.clone()));
        let access = Arc::new(IamAccessService::new(
            iam_repository,
            core.iam_platform_administrators.clone(),
            license.license.clone(),
        ));
        let email_domains: Arc<dyn EmailDomainRepository> =
            Arc::new(PgEmailDomainRepository::new(core.pool.clone()));
        let audit_events: Arc<dyn AuditEventRepository> =
            Arc::new(PgAuditEventRepository::new(core.pool.clone()));

        let sso_state_store = Arc::new(SsoStateStore::new(600));
        let sso_jwks_cache = Arc::new(
            JwksCache::new(3600)
                .map_err(|error| Error::internal(format!("init jwks cache: {error}")))?,
        );
        let sso_sessions: Arc<dyn SsoSessionRepository> =
            Arc::new(PgSsoSessionRepository::new(core.pool.clone()));
        let sso_providers: Arc<dyn SsoProviderRepository> =
            Arc::new(PgSsoProviderRepository::new(core.pool.clone()));

        Ok(Self {
            iam,
            access,
            instance_settings,
            signing_secrets,
            api_tokens,
            user_preferences,
            workspace_preference_defaults,
            invitations,
            roles,
            email_domains,
            audit_events,
            sso_state_store,
            sso_jwks_cache,
            sso_sessions,
            sso_providers,
        })
    }
}

pub(super) async fn seed_root_if_needed(
    iam: &Arc<IamService>,
    orgs: &PgOrganizationRepository,
    iam_memberships: &PgIamMembershipRepository,
    settings: &Settings,
) -> Result<()> {
    if settings.auth.root_email.is_empty() || settings.auth.root_password.is_empty() {
        return Ok(());
    }
    if iam.users.count().await? > 0 {
        return Ok(());
    }
    let org = match orgs.get_by_slug("default").await {
        Ok(org) => org,
        Err(Error::NotFound(_)) => {
            orgs.create(Organization {
                id: Id::new(),
                name: "default".into(),
                slug: "default".into(),
                system: false,
                disabled: false,
                created_at: TimestampMicros::now(),
            })
            .await?
        }
        Err(error) => return Err(error),
    };
    let user = iam
        .create_user(
            settings.auth.root_email.clone(),
            "root".into(),
            &settings.auth.root_password,
        )
        .await?;
    let role_id = iam_memberships
        .role_id_for_purpose(&org.id, "organization_bootstrap")
        .await?;
    iam_memberships
        .upsert(
            IamMembership {
                user_id: user.id.clone(),
                org_id: org.id,
                joined_at: TimestampMicros::now(),
            },
            &[role_id],
            &user.id,
        )
        .await?;
    tracing::info!(email = %settings.auth.root_email, "seeded root user + default org");
    Ok(())
}

pub(super) async fn bootstrap_platform_administrator(
    iam: &IamService,
    iam_platform_administrators: &dyn IamPlatformAdministratorRepository,
    settings: &Settings,
) -> Result<()> {
    let root_email = settings.auth.root_email.trim();
    if root_email.is_empty() {
        return Ok(());
    }
    let root = iam.users.get_by_email(root_email).await.map_err(|error| {
        if matches!(error, Error::NotFound(_)) {
            Error::invalid("configured root user does not exist for platform bootstrap")
        } else {
            error
        }
    })?;
    if iam_platform_administrators.bootstrap_root(&root.id).await? {
        tracing::info!(
            user_id = %root.id.0,
            "reconciled configured root as the only platform administrator"
        );
    }
    Ok(())
}
