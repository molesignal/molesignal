// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Twilio 的电话（[`voice`]）与短信（[`sms`]）：REST API（2010-04-01 版），以 Account SID 与
//! Auth Token 做 HTTP Basic 认证，表单提交。电话念的话直接写在 TwiML 里，不用审核模板。

mod sms;
mod voice;

use serde::Deserialize;
use serde_json::Value;
pub use sms::{TWILIO_SMS_CONNECTOR_TYPE, TwilioSmsConnectorAdapter};
pub use voice::{TWILIO_VOICE_CONNECTOR_TYPE, TwilioVoiceConnectorAdapter};

use super::common::{self, default_timeout_secs};
use crate::{
    domain::notify::connector::ConnectorAddress,
    shared::{
        Error, Result,
        http_trace::{self, HttpTarget},
    },
};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Account {
    #[serde(default)]
    account_sid: String,
    #[serde(default)]
    auth_token: String,
    #[serde(default = "default_api_base_url")]
    api_base_url: String,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
}

const TWILIO_API_BASE_URL: &str = "https://api.twilio.com";

fn default_api_base_url() -> String {
    TWILIO_API_BASE_URL.into()
}

/// 电话与短信都发往 Twilio 的这个地址。
fn address() -> ConnectorAddress {
    ConnectorAddress::fixed("apiBaseUrl", TWILIO_API_BASE_URL)
}

impl Account {
    fn validate(&self, connector: &str) -> Result<()> {
        if !is_sid(self.account_sid.trim(), "AC") {
            return Err(Error::invalid(format!(
                "{connector} account_sid must be the account's SID (AC…)"
            )));
        }

        common::require(&self.auth_token, &format!("{connector} authToken"))?;
        common::validate_http_url(&self.api_base_url, &format!("{connector} apiBaseUrl"))?;
        common::validate_timeout(self.timeout_secs, connector)
    }
}

/// Twilio 的 SID：两个字母的前缀加 32 位十六进制。
fn is_sid(value: &str, prefix: &str) -> bool {
    value.len() == 34
        && value.starts_with(prefix)
        && value[2..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// 在账号下建一个资源（`Calls`、`Messages`），返回它的 SID。
async fn create(
    connector: &str,
    account: &Account,
    resource: &str,
    form: &[(&str, String)],
) -> Result<Option<String>> {
    let url = format!(
        "{}/2010-04-01/Accounts/{}/{resource}.json",
        account.api_base_url.trim().trim_end_matches('/'),
        account.account_sid.trim()
    );

    let client = common::client(account.timeout_secs, connector)?;

    let request = client
        .post(url)
        .basic_auth(account.account_sid.trim(), Some(account.auth_token.trim()))
        .form(form);

    let response = http_trace::send(&client, request, HttpTarget::ThirdParty)
        .await
        .map_err(|_| Error::unavailable(format!("{connector} request failed")))?;

    let status = response.status();
    let answer = response.json::<Value>().await.unwrap_or(Value::Null);

    if status.is_success() {
        return Ok(answer
            .get("sid")
            .and_then(Value::as_str)
            .map(str::to_string));
    }

    let code = answer
        .get("code")
        .filter(|code| !code.is_null())
        .map(|code| format!(" {code}"))
        .unwrap_or_default();

    let message = answer
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();

    Err(Error::unavailable(format!(
        "{connector} was refused: HTTP {}{code}: {}",
        status.as_u16(),
        common::provider_message(message)
    )))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    pub(super) const SID: &str = concat!("AC", "00000000000000000000000000000000");

    #[test]
    fn the_account_needs_its_sid_and_token() {
        let account = |config: Value| serde_json::from_value::<Account>(config).unwrap();

        assert!(
            account(json!({"accountSid": SID, "authToken": "t"}))
                .validate("twilio_sms")
                .is_ok()
        );
        assert!(
            account(json!({"accountSid": "AC123", "authToken": "t"}))
                .validate("twilio_sms")
                .is_err()
        );
        assert!(
            account(json!({"accountSid": SID}))
                .validate("twilio_sms")
                .is_err()
        );
    }
}
