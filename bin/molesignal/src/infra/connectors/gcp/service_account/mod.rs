// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A Google Cloud service account key, as the connectors that call Google APIs take it under
//! `serviceAccountKey`, and the access tokens it signs for ([`token`]).

use kernel::{Error, Result};
use serde::Deserialize;
use serde_json::Value;

use crate::infra::connectors::{ConnectorKind, cloud_metrics};

mod token;

pub use token::TokenSource;
#[cfg(test)]
pub(crate) use token::test_key;

/// Google's token endpoints. A key's `token_uri` must be one of them, so a signed assertion is
/// never sent anywhere else. Older keys carry the second one.
const GOOGLE_TOKEN_URIS: [&str; 2] = [
    "https://oauth2.googleapis.com/token",
    "https://accounts.google.com/o/oauth2/token",
];

/// The parts of a service account key file (JSON) that signing in and reading need.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ServiceAccount {
    pub client_email: String,
    pub private_key: String,
    #[serde(default)]
    pub token_uri: String,
    #[serde(default)]
    pub project_id: String,
}

impl ServiceAccount {
    /// The key under `serviceAccountKey` of a connector of `kind`: the JSON file Google hands
    /// out, as text or as an object.
    pub fn parse(config: &Value, kind: ConnectorKind) -> Result<Self> {
        let invalid = || {
            Error::invalid(
                "`service_account_key` must be a service account key file (JSON) with client_email \
                 and private_key",
            )
        };

        let key = match config.get("serviceAccountKey") {
            Some(Value::Object(_)) => config["serviceAccountKey"].clone(),
            Some(Value::String(_)) => {
                serde_json::from_str(cloud_metrics::required(config, "serviceAccountKey", kind)?)
                    .map_err(|_| invalid())?
            }
            _ => {
                cloud_metrics::required(config, "serviceAccountKey", kind)?;
                return Err(invalid());
            }
        };

        let mut account: ServiceAccount = serde_json::from_value(key).map_err(|_| invalid())?;

        account.client_email = account.client_email.trim().to_string();
        account.token_uri = match account.token_uri.trim().trim_end_matches('/') {
            "" => GOOGLE_TOKEN_URIS[0].to_string(),
            uri => uri.to_string(),
        };

        if account.client_email.is_empty() || account.private_key.trim().is_empty() {
            return Err(invalid());
        }

        if !GOOGLE_TOKEN_URIS.contains(&account.token_uri.as_str()) {
            return Err(Error::invalid(
                "the token_uri of `service_account_key` must be Google's token endpoint",
            ));
        }

        token::signing_key(&account.private_key)?;
        Ok(account)
    }
}
