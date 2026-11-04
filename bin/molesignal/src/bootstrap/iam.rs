// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! IAM、认证、SSO 与安全审计运行时装配。

use std::sync::Arc;

use super::{
    core::Core,
    workers::jwt_secrets::{self, SigningSecretsTable},
};
use crate::{
    app::{
        audit_stream::{AuditStreamQueue, StreamedAuditEvents},
        iam::{IamAccessService, IamService},
    },
    config::Settings,
    domain::iam::{
        IamMembership, IamMembershipRepository, IamPlatformAdministratorRepository,
        InstanceSettingsRepository, Organization, OrganizationRepository, SsoProviderRepository,
        api_token::ApiTokenRepository, service_account::ServiceAccountRepository,
    },
    infra::{
        persistence::repositories::{
            api_tokens::PgApiTokenRepository,
            audit_events::{AuditEventRepository, PgAuditEventRepository},
            email_domains::{EmailDomainRepository, PgEmailDomainRepository},
            iam::{
                PgIamRepository,
                memberships::PgIamMembershipRepository,
                policies::{IamPolicyRepository, PgIamPolicyRepository},
                roles::{IamRoleRepository, PgIamRoleRepository},
                service_accounts::PgServiceAccountRepository,
            },
            instance_settings::PgInstanceSettingsRepository,
            invitations::{InvitationRepository, PgInvitationRepository},
            organizations::PgOrganizationRepository,
            signing_secrets::{
                PgSigningSecretRepository, SigningSecretRepository, bootstrap_or_load_jwt_secret,
            },
            sso_providers::PgSsoProviderRepository,
            user_preferences::{PgUserPreferencesRepository, UserPreferencesRepository},
            user_ui_preference::{PgUserUiPreferenceRepository, UserUiPreferenceRepository},
            workspace_preference_defaults::{
                PgWorkspacePreferenceDefaultsRepository, WorkspacePreferenceDefaultsRepository,
            },
        },
        sso::{
            JwksCache, PgSsoIdentityRepository, PgSsoLinkRequestRepository,
            PgSsoSigninTestRepository, SsoIdentityRepository, SsoLinkRequestRepository,
            SsoSigninTestRepository, SsoStateStore,
        },
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) struct IamRuntime {
    pub(super) iam: Arc<IamService>,
    pub(super) access: Arc<IamAccessService>,
    pub(super) instance_settings: Arc<dyn InstanceSettingsRepository>,
    pub(super) signing_secrets: Arc<dyn SigningSecretRepository>,
    pub(super) api_tokens: Arc<dyn ApiTokenRepository>,
    pub(super) service_accounts: Arc<dyn ServiceAccountRepository>,
    pub(super) user_preferences: Arc<dyn UserPreferencesRepository>,
    pub(super) user_ui_preference: Arc<dyn UserUiPreferenceRepository>,
    pub(super) workspace_preference_defaults: Arc<dyn WorkspacePreferenceDefaultsRepository>,
    pub(super) invitations: Arc<dyn InvitationRepository>,
    pub(super) roles: Arc<dyn IamRoleRepository>,
    pub(super) policies: Arc<dyn IamPolicyRepository>,
    pub(super) email_domains: Arc<dyn EmailDomainRepository>,
    pub(super) audit_events: Arc<dyn AuditEventRepository>,
    pub(super) sso_state_store: Arc<SsoStateStore>,
    pub(super) sso_jwks_cache: Arc<JwksCache>,
    pub(super) sso_identities: Arc<dyn SsoIdentityRepository>,
    pub(super) sso_link_requests: Arc<dyn SsoLinkRequestRepository>,
    pub(super) sso_signin_tests: Arc<dyn SsoSigninTestRepository>,
    pub(super) sso_providers: Arc<dyn SsoProviderRepository>,
}

impl IamRuntime {
    /// `audit_stream` takes every audit event to the organization's `_audit` stream as well.
    pub(super) async fn build(
        settings: &Settings,
        core: &Core,
        audit_stream: AuditStreamQueue,
    ) -> Result<Self> {
        let signing_secrets: Arc<dyn SigningSecretRepository> = Arc::new(
            PgSigningSecretRepository::new(core.pool.clone(), core.cipher_root_key.clone()),
        );
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

        let iam = Arc::new(
            IamService::new(
                core.users.clone(),
                core.orgs.clone(),
                core.iam_memberships.clone(),
                settings.auth.clone(),
                jwt_active_secrets,
            )
            .with_jwt_secret_source(Arc::new(SigningSecretsTable(signing_secrets.clone()))),
        );

        let _jwt_secrets_handle = jwt_secrets::spawn(iam.clone());

        let api_tokens: Arc<dyn ApiTokenRepository> = Arc::new(
            PgApiTokenRepository::new(core.pool.clone()).with_cipher(core.cipher_root_key.clone()),
        );

        let service_accounts: Arc<dyn ServiceAccountRepository> =
            Arc::new(PgServiceAccountRepository::new(core.pool.clone()));
        let user_preferences: Arc<dyn UserPreferencesRepository> =
            Arc::new(PgUserPreferencesRepository::new(core.pool.clone()));
        let user_ui_preference: Arc<dyn UserUiPreferenceRepository> =
            Arc::new(PgUserUiPreferenceRepository::new(core.pool.clone()));

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

        // An organization's owner holds the organization's whole catalog and nothing of the
        // platform's. Reconciled here, after migrations, so a permission a migration adds reaches
        // the owners without naming them.
        let owner_roles = roles.grant_catalog_to_organization_owners().await?;

        if owner_roles > 0 {
            tracing::info!(
                changed = owner_roles,
                "organization owner role reconciled with the permission catalog"
            );
        }

        // A custom policy that names permissions by pattern also allows the ones added since.
        let policies: Arc<dyn IamPolicyRepository> =
            Arc::new(PgIamPolicyRepository::new(core.pool.clone()));
        let refreshed = policies.refresh_patterns().await?;

        if refreshed > 0 {
            tracing::info!(
                refreshed,
                "permission policies resolved again against the permission catalog"
            );
        }

        let iam_repository: Arc<dyn crate::domain::iam::access::IamRepository> =
            Arc::new(PgIamRepository::new(core.pool.clone()));

        let access = Arc::new(IamAccessService::new(
            iam_repository,
            core.iam_platform_administrators.clone(),
        ));

        let email_domains: Arc<dyn EmailDomainRepository> =
            Arc::new(PgEmailDomainRepository::new(core.pool.clone()));
        let audit_events: Arc<dyn AuditEventRepository> = Arc::new(StreamedAuditEvents::new(
            Arc::new(PgAuditEventRepository::new(core.pool.clone())),
            audit_stream,
        ));

        let sso_state_store = Arc::new(SsoStateStore::new(
            core.pool.clone(),
            core.cipher_root_key.clone(),
            600,
        ));

        let sso_jwks_cache = Arc::new(
            JwksCache::new(3600)
                .map_err(|error| Error::internal(format!("init jwks cache: {error}")))?,
        );

        let sso_identities: Arc<dyn SsoIdentityRepository> =
            Arc::new(PgSsoIdentityRepository::new(core.pool.clone()));
        let sso_link_requests: Arc<dyn SsoLinkRequestRepository> =
            Arc::new(PgSsoLinkRequestRepository::new(core.pool.clone()));
        let sso_signin_tests: Arc<dyn SsoSigninTestRepository> =
            Arc::new(PgSsoSigninTestRepository::new(core.pool.clone()));
        let sso_providers: Arc<dyn SsoProviderRepository> =
            Arc::new(PgSsoProviderRepository::new(core.pool.clone()));

        Ok(Self {
            iam,
            access,
            instance_settings,
            signing_secrets,
            api_tokens,
            service_accounts,
            user_preferences,
            user_ui_preference,
            workspace_preference_defaults,
            invitations,
            roles,
            policies,
            email_domains,
            audit_events,
            sso_state_store,
            sso_jwks_cache,
            sso_identities,
            sso_link_requests,
            sso_signin_tests,
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

    // Root reaches every organization without a role. It belongs to this one so that it has
    // somewhere to land after signing in.
    iam_memberships
        .upsert(
            IamMembership {
                user_id: user.id.clone(),
                org_id: org.id,
                joined_at: TimestampMicros::now(),
            },
            &[],
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
