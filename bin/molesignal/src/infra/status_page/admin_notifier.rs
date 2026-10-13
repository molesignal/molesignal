// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Arc;

use async_trait::async_trait;
use sqlx::PgPool;

use crate::{
    domain::{
        i18n_language::{I18nLanguageRepository, LanguagePack},
        status_page::{StatusPageDomainHealthAlert, StatusPageDomainHealthNotifier},
    },
    infra::{
        notify::EmailSender,
        persistence::repositories::workspace_preference_defaults::{
            PgWorkspacePreferenceDefaultsRepository, WorkspacePreferenceDefaultsRepository,
        },
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// The alert and the domain state names it uses sit under this key prefix.
const LANGUAGE_KEY_PREFIX: &str = "status_page.";

pub struct StatusPageAdminEmailNotifier {
    pool: PgPool,
    email: Arc<EmailSender>,
    i18n_language: Arc<dyn I18nLanguageRepository>,
    external_url: String,
}

impl StatusPageAdminEmailNotifier {
    pub fn new(
        pool: PgPool,
        email: Arc<EmailSender>,
        i18n_language: Arc<dyn I18nLanguageRepository>,
        external_url: impl Into<String>,
    ) -> Self {
        Self {
            pool,
            email,
            i18n_language,
            external_url: external_url.into(),
        }
    }

    /// The copy of the alert, in the default language of the workspace that owns the page.
    async fn copy(&self, org_id: &Id) -> Result<LanguagePack> {
        let workspace = PgWorkspacePreferenceDefaultsRepository::new(self.pool.clone())
            .get(org_id)
            .await?;

        self.i18n_language
            .load_pack(LANGUAGE_KEY_PREFIX, &workspace.language)
            .await
    }
}

#[async_trait]
impl StatusPageDomainHealthNotifier for StatusPageAdminEmailNotifier {
    async fn notify(&self, alert: &StatusPageDomainHealthAlert) -> Result<()> {
        let now = TimestampMicros::now().0;

        let recipients: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT user_row.email
             FROM iam_role_bindings binding
             JOIN iam_roles role
               ON role.org_id = binding.organization_id AND role.id = binding.role_id
             JOIN iam_role_permissions permission
               ON permission.role_id = role.id
              AND permission.permission_key = 'status_pages.manage'
             JOIN iam_memberships membership
               ON membership.org_id = binding.organization_id
              AND membership.user_id = binding.principal_id
             JOIN users user_row ON user_row.id = membership.user_id
             WHERE binding.organization_id = $1
               AND binding.principal_type = 'user'
               AND binding.resource_type IS NULL AND binding.resource_id IS NULL
               AND (binding.starts_at_micros IS NULL OR binding.starts_at_micros <= $2)
               AND (binding.expires_at_micros IS NULL OR binding.expires_at_micros > $2)
               AND NOT user_row.disabled AND user_row.status = 'active'
             ORDER BY user_row.email",
        )
        .bind(&alert.org_id.0)
        .bind(now)
        .fetch_all(&self.pool)
        .await
        .map_err(|error| Error::internal(format!("status-page admin lookup: {error}")))?;

        if recipients.is_empty() {
            return Err(Error::unavailable(
                "no active status-page administrator can receive the domain alert",
            ));
        }

        let settings_url = format!(
            "{}/status-pages/{}/settings/domain-access",
            self.external_url.trim().trim_end_matches('/'),
            alert.status_page_id
        );

        let copy = self.copy(&alert.org_id).await?;
        let (subject, body) = written(&copy, alert, &settings_url)?;

        for recipient in recipients {
            self.email.send_text(&[recipient], &subject, &body).await?;
        }

        Ok(())
    }
}

/// Subject and body of the alert. A state without a name in `copy` keeps its stored value.
fn written(
    copy: &LanguagePack,
    alert: &StatusPageDomainHealthAlert,
    settings_url: &str,
) -> Result<(String, String)> {
    let state = alert.state.as_str();

    let state_name = copy
        .get(&format!("{LANGUAGE_KEY_PREFIX}domain_state.{state}"))
        .unwrap_or(state);

    let detail = match alert.error.as_deref() {
        Some(error) => error.chars().take(500).collect::<String>(),
        None => copy.render("status_page.email.domain_alert.no_detail", &[])?,
    };

    let fills = [
        ("page_name", alert.page_name.as_str()),
        ("hostname", alert.hostname.as_str()),
        ("state", state_name),
        ("detail", detail.as_str()),
        ("settings_url", settings_url),
    ];

    Ok((
        copy.render("status_page.email.domain_alert.subject", &fills)?,
        copy.render("status_page.email.domain_alert.body", &fills)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::status_page::StatusPageDomainState, shared::i18n_language_seed::seeded_pack,
    };

    fn alert(error: Option<&str>) -> StatusPageDomainHealthAlert {
        StatusPageDomainHealthAlert {
            org_id: Id("org".into()),
            status_page_id: Id("page".into()),
            page_name: "Acme Cloud".into(),
            hostname: "status.acme.example".into(),
            state: StatusPageDomainState::Degraded,
            error: error.map(str::to_string),
        }
    }

    #[test]
    fn domain_alert_is_written_in_the_workspace_language() {
        let url = "https://molesignal.example/status-pages/page/settings/domain-access";

        let (subject, body) = written(
            &seeded_pack(LANGUAGE_KEY_PREFIX, "en-us"),
            &alert(Some("TLS certificate expires in 3 days")),
            url,
        )
        .unwrap();
        assert_eq!(subject, "Acme Cloud custom domain needs attention");
        assert_eq!(
            body,
            "The custom domain status.acme.example for Acme Cloud needs attention.\nState: Degraded\n\nTLS certificate expires in 3 days\n\nReview Domain & Access: https://molesignal.example/status-pages/page/settings/domain-access"
        );

        let (subject, body) = written(
            &seeded_pack(LANGUAGE_KEY_PREFIX, "zh-cn"),
            &alert(None),
            url,
        )
        .unwrap();
        assert_eq!(subject, "Acme Cloud 的自定义域名需要处理");
        assert_eq!(
            body,
            "Acme Cloud 的自定义域名 status.acme.example 需要处理。\n当前状态：异常\n\n最近一次域名健康检查没有给出详细信息。\n\n前往“域名与访问”处理：https://molesignal.example/status-pages/page/settings/domain-access"
        );
    }
}
