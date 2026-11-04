// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 按身份字段映射从 JSON 声明（ID Token、UserInfo、平台用户接口）里取身份。
//!
//! 映射值是 claim 名或点分路径；可用 `|` 列出几个来源，依次取第一个有值的，如
//! `email | preferred_username`。整条路径恰好是一个 key（如 Auth0 的
//! `https://example.com/roles`）时直接按 key 取。

use serde_json::Value;

use crate::{
    domain::iam::SsoFieldMapping,
    shared::{Error, Result},
};

/// 从外部身份源取出、尚未落到本地账号的身份。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalIdentity {
    pub subject: String,
    pub email: String,
    pub name: Option<String>,
    pub groups: Vec<String>,
    /// 身份源对邮箱是否已验证的声明；没有声明时为 `None`。
    pub email_verified: Option<bool>,
}

pub fn map_identity(claims: &Value, mapping: &SsoFieldMapping) -> Result<ExternalIdentity> {
    let subject = claim_string(claims, &mapping.subject).ok_or_else(|| {
        Error::unauthorized(format!(
            "identity is missing the mapped subject claim `{}`",
            mapping.subject
        ))
    })?;

    let email = claim_string(claims, &mapping.email).ok_or_else(|| {
        Error::unauthorized(format!(
            "identity is missing the mapped email claim `{}`",
            mapping.email
        ))
    })?;

    let mut groups = claim_strings(claims, &mapping.groups);
    groups.sort();
    groups.dedup();

    Ok(ExternalIdentity {
        subject,
        email,
        name: claim_string(claims, &mapping.display_name),
        groups,
        email_verified: email_verified(claims),
    })
}

/// 标准 `email_verified` 声明；有的身份源把布尔值写成字符串。
pub fn email_verified(claims: &Value) -> Option<bool> {
    match claims.get("email_verified")? {
        Value::Bool(verified) => Some(*verified),
        Value::String(text) if text.eq_ignore_ascii_case("true") => Some(true),
        Value::String(text) if text.eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    }
}

/// 映射值里的各个来源，去掉空白。
pub fn claim_sources(mapping: &str) -> impl Iterator<Item = &str> {
    mapping.split('|').map(str::trim)
}

fn claim_at_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    value.get(path).or_else(|| {
        path.split('.')
            .try_fold(value, |current, segment| current.get(segment))
    })
}

fn claim_string(value: &Value, mapping: &str) -> Option<String> {
    claim_sources(mapping).find_map(|path| {
        let text = match claim_at_path(value, path)? {
            Value::String(text) => text.trim().to_owned(),
            Value::Number(number) => number.to_string(),
            _ => return None,
        };

        (!text.is_empty()).then_some(text)
    })
}

fn claim_strings(value: &Value, mapping: &str) -> Vec<String> {
    claim_sources(mapping)
        .map(|path| match claim_at_path(value, path) {
            Some(Value::Array(values)) => values
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .collect(),
            Some(Value::String(value)) if !value.trim().is_empty() => {
                vec![value.trim().to_owned()]
            }
            _ => Vec::new(),
        })
        .find(|groups| !groups.is_empty())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn nested_paths_and_numeric_subjects_are_read() {
        let claims = json!({
            "id": 583231,
            "mail": "alice@example.com",
            "profile": { "display": "Alice" },
            "realm_access": { "roles": ["operator"] }
        });
        let mapping = SsoFieldMapping {
            subject: "id".into(),
            email: "mail".into(),
            display_name: "profile.display".into(),
            groups: "realm_access.roles".into(),
        };
        let identity = map_identity(&claims, &mapping).expect("mapped identity");
        assert_eq!(identity.subject, "583231");
        assert_eq!(identity.name.as_deref(), Some("Alice"));
        assert_eq!(identity.groups, ["operator"]);
        assert_eq!(identity.email_verified, None);
    }

    #[test]
    fn alternatives_take_the_first_source_with_a_value() {
        let claims = json!({
            "sub": "u-1",
            "email": "",
            "preferred_username": "alice@corp.example.com",
            "roles": ["admins"],
            "email_verified": "true"
        });
        let mapping = SsoFieldMapping {
            subject: "oid | sub".into(),
            email: "email | preferred_username".into(),
            display_name: "name".into(),
            groups: "groups|roles".into(),
        };
        let identity = map_identity(&claims, &mapping).expect("mapped identity");
        assert_eq!(identity.subject, "u-1");
        assert_eq!(identity.email, "alice@corp.example.com");
        assert_eq!(identity.groups, ["admins"]);
        assert_eq!(identity.email_verified, Some(true));
    }

    #[test]
    fn whole_key_paths_win_over_dotted_lookup() {
        let claims = json!({
            "sub": "u-1",
            "email": "a@example.com",
            "https://example.com/roles": ["sre", "sre", "dev"]
        });
        let mapping = SsoFieldMapping {
            groups: "https://example.com/roles".into(),
            ..SsoFieldMapping::oidc()
        };
        let identity = map_identity(&claims, &mapping).expect("mapped identity");
        assert_eq!(identity.groups, ["dev", "sre"]);
    }

    #[test]
    fn missing_email_is_rejected() {
        let error = map_identity(&json!({ "sub": "u-1" }), &SsoFieldMapping::oidc())
            .expect_err("email is required");
        assert!(error.to_string().contains("email"));
    }
}
