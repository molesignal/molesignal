// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! OAuth 2.0 sign-in for SMTP: the access token a mail server takes with SASL XOAUTH2.
//!
//! - Microsoft 365: the client credentials grant of an app registered in Microsoft Entra ID with
//!   the `SMTP.SendAsApp` permission, for the Exchange Online scope. The tenant admin registers the
//!   app's service principal in Exchange and gives it the sending mailbox. Only the worldwide
//!   cloud documents app-only SMTP; the national clouds are left out until they do.
//! - Google Workspace: a service account with domain-wide delegation acts as the sending mailbox,
//!   scope `https://mail.google.com/`.
//!
//! Tokens are cached until a minute before they expire and dropped when a send fails, so the
//! next send signs in again.

use kernel::{Error, Result};
use serde::Deserialize;
use signals::http_trace::{self, HttpTarget};

use super::{
    super::{
        common::{self, TOKENS, TokenCache, provider_message},
        google_account::ServiceAccount,
    },
    EMAIL_SMTP_CONNECTOR_TYPE, EmailSmtpConfig,
};

/// Where Microsoft Entra ID signs in.
pub(super) const MICROSOFT_LOGIN_BASE_URL: &str = "https://login.microsoftonline.com";

/// The scope app-only SMTP must ask for.
const EXCHANGE_SCOPE: &str = "https://outlook.office365.com/.default";

const GMAIL_SCOPE: &str = "https://mail.google.com/";

#[derive(Debug, Default, Deserialize)]
struct TokenResponse {
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    expires_in: u64,
    #[serde(default)]
    error: String,
    #[serde(default)]
    error_description: String,
}

fn microsoft_key(config: &EmailSmtpConfig) -> String {
    TokenCache::key(
        &format!("{EMAIL_SMTP_CONNECTOR_TYPE}:microsoft"),
        &format!(
            "{}:{}:{}",
            config.login_base_url.trim(),
            config.tenant_id.trim(),
            config.client_id.trim()
        ),
        &config.client_secret,
    )
}

fn google_key(config: &EmailSmtpConfig, account: &ServiceAccount) -> String {
    TokenCache::key(
        &format!("{EMAIL_SMTP_CONNECTOR_TYPE}:google"),
        &format!("{}:{}", account.client_email.trim(), config.username.trim()),
        &account.private_key,
    )
}

/// An app-only token for Exchange Online.
pub(super) async fn microsoft_token(config: &EmailSmtpConfig) -> Result<String> {
    TOKENS
        .get(&microsoft_key(config), async {
            let http = common::client(config.timeout_secs.into(), EMAIL_SMTP_CONNECTOR_TYPE)?;
            let url = format!(
                "{}/{}/oauth2/v2.0/token",
                config.login_base_url.trim().trim_end_matches('/'),
                config.tenant_id.trim()
            );

            let response = http_trace::send(
                &http,
                http.post(url).form(&[
                    ("grant_type", "client_credentials"),
                    ("client_id", config.client_id.trim()),
                    ("client_secret", config.client_secret.trim()),
                    ("scope", EXCHANGE_SCOPE),
                ]),
                HttpTarget::ThirdParty,
            )
            .await
            .map_err(|_| {
                Error::unavailable("email_smtp could not reach Microsoft Entra ID to sign in")
            })?;

            let token: TokenResponse = response.json().await.unwrap_or_default();

            if token.access_token.is_empty() {
                let reason = if token.error_description.is_empty() {
                    &token.error
                } else {
                    &token.error_description
                };

                return Err(Error::unavailable(format!(
                    "Microsoft Entra ID refused the email_smtp sign-in: {}",
                    provider_message(reason)
                )));
            }

            Ok((token.access_token, token.expires_in.max(60)))
        })
        .await
}

/// A token for Gmail, acting as the sending mailbox.
pub(super) async fn google_token(config: &EmailSmtpConfig) -> Result<String> {
    let account = ServiceAccount::parse(&config.credentials, EMAIL_SMTP_CONNECTOR_TYPE)?;
    let http = common::client(config.timeout_secs.into(), EMAIL_SMTP_CONNECTOR_TYPE)?;

    TOKENS
        .get(
            &google_key(config, &account),
            account.fetch_token(
                &http,
                GMAIL_SCOPE,
                Some(config.username.trim()),
                EMAIL_SMTP_CONNECTOR_TYPE,
            ),
        )
        .await
}

/// Drops the cached token, so the next send signs in again.
pub(super) fn forget(config: &EmailSmtpConfig) {
    TOKENS.forget(&microsoft_key(config));

    if let Ok(account) = ServiceAccount::parse(&config.credentials, EMAIL_SMTP_CONNECTOR_TYPE) {
        TOKENS.forget(&google_key(config, &account));
    }
}
