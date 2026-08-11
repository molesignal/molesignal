// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Arc;

use async_trait::async_trait;
use sqlx::PgPool;

use crate::{
    domain::status_page::{StatusPageDomainHealthAlert, StatusPageDomainHealthNotifier},
    infra::notify::EmailSender,
    shared::{Error, Result, time::TimestampMicros},
};

pub struct StatusPageAdminEmailNotifier {
    pool: PgPool,
    email: Arc<EmailSender>,
    external_url: String,
}

impl StatusPageAdminEmailNotifier {
    pub fn new(pool: PgPool, email: Arc<EmailSender>, external_url: impl Into<String>) -> Self {
        Self {
            pool,
            email,
            external_url: external_url.into(),
        }
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
        let detail = alert
            .error
            .as_deref()
            .map(|value| value.chars().take(500).collect::<String>())
            .unwrap_or_else(|| "The latest domain health check did not provide details.".into());
        let subject = format!("{} custom domain needs attention", alert.page_name);
        let body = format!(
            "The custom domain {hostname} for {page} is {state}.\n\n{detail}\n\nReview Domain & Access: {settings_url}",
            hostname = alert.hostname,
            page = alert.page_name,
            state = alert.state.as_str(),
        );
        for recipient in recipients {
            self.email.send_text(&[recipient], &subject, &body).await?;
        }
        Ok(())
    }
}
