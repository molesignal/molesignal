// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! MCP 服务器的请求头与环境变量。
//!
//! 名称里有 key、secret、token、password、credential、auth、cookie 这类词的值算敏感：加密
//! 保存，读出来只有末四位，修改时留空表示不变。别的值也可以手动标成敏感；自动认出的敏感值
//! 不能改回明文。

use std::collections::HashSet;

use reqwest::header::{HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result};

/// 名称里出现这些词（不分大小写）时值算敏感。
const SECRET_PARTS: &[&str] = &[
    "key",
    "secret",
    "token",
    "password",
    "passwd",
    "credential",
    "auth",
    "cookie",
    "signature",
];

/// 由 MCP 客户端自己设置、不能被覆盖的请求头。
const RESERVED_HEADERS: &[&str] = &[
    "host",
    "content-length",
    "content-type",
    "accept",
    "connection",
    "transfer-encoding",
    "te",
    "trailer",
    "upgrade",
    "keep-alive",
    "proxy-authorization",
    "proxy-connection",
    "mcp-session-id",
    "mcp-protocol-version",
    "last-event-id",
];

const MAX_VALUES: usize = 32;
const MAX_NAME_CHARS: usize = 128;
const MAX_VALUE_BYTES: usize = 8 * 1024;

pub fn looks_secret(name: &str) -> bool {
    let name = name.to_ascii_lowercase();

    SECRET_PARTS.iter().any(|part| name.contains(part))
}

/// 末四个字符，掩码显示用。
pub fn last4(value: &str) -> String {
    let characters: Vec<char> = value.chars().collect();

    characters[characters.len().saturating_sub(4)..]
        .iter()
        .collect()
}

/// 保存下来的一个请求头或环境变量。敏感的只有是否已填与末四位。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedValue {
    pub name: String,
    pub secret: bool,
    /// 不敏感时的值。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// 敏感值的末四位。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last4: Option<String>,
    /// 敏感值已经填了。
    #[serde(default)]
    pub set: bool,
}

/// 写入时的一项：敏感值为 `None` 表示保留原来的值（原来没有就是还没填）。
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct NamedValueInput {
    pub name: String,
    #[serde(default)]
    pub secret: bool,
    #[serde(default)]
    pub value: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    Header,
    Env,
}

impl ValueKind {
    fn label(self) -> &'static str {
        match self {
            Self::Header => "header",
            Self::Env => "environment variable",
        }
    }
}

fn valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();

    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// 校验并整理：请求头名转小写；同名只能有一个；名称像敏感的一律按敏感存；不敏感的必须填值。
pub fn normalize_values(
    kind: ValueKind,
    items: Vec<NamedValueInput>,
) -> Result<Vec<NamedValueInput>> {
    if items.len() > MAX_VALUES {
        return Err(Error::invalid(format!(
            "at most {MAX_VALUES} {}s are allowed",
            kind.label()
        )));
    }

    let mut seen = HashSet::new();
    let mut normalized = Vec::with_capacity(items.len());

    for item in items {
        let name = item.name.trim();

        if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
            return Err(Error::invalid(format!(
                "a {} name must contain 1 to {MAX_NAME_CHARS} characters",
                kind.label()
            )));
        }

        let name = match kind {
            ValueKind::Header => {
                let lower = name.to_ascii_lowercase();

                HeaderName::from_bytes(lower.as_bytes())
                    .map_err(|_| Error::invalid(format!("`{name}` is not a valid header name")))?;

                if RESERVED_HEADERS.contains(&lower.as_str()) {
                    return Err(Error::invalid(format!(
                        "the `{lower}` header is set by Mole Agent and cannot be configured"
                    )));
                }

                lower
            }
            ValueKind::Env if valid_env_name(name) => name.to_string(),
            ValueKind::Env => {
                return Err(Error::invalid(format!(
                    "`{name}` is not a valid environment variable name"
                )));
            }
        };

        if !seen.insert(name.to_ascii_lowercase()) {
            return Err(Error::invalid(format!(
                "the {} `{name}` is listed twice",
                kind.label()
            )));
        }

        let secret = item.secret || looks_secret(&name);

        let value = item
            .value
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        if let Some(value) = &value {
            if value.len() > MAX_VALUE_BYTES {
                return Err(Error::invalid(format!(
                    "the value of `{name}` is longer than {MAX_VALUE_BYTES} bytes"
                )));
            }

            if kind == ValueKind::Header && HeaderValue::from_str(value).is_err() {
                return Err(Error::invalid(format!(
                    "the value of `{name}` contains characters a header cannot carry"
                )));
            }
        }

        if !secret && value.is_none() {
            return Err(Error::invalid(format!("`{name}` needs a value")));
        }

        normalized.push(NamedValueInput {
            name,
            secret,
            value,
        });
    }

    Ok(normalized)
}

/// 两组里有同名的（不分大小写）时返回它。
pub fn find_reserved_overlap<'a>(values: &'a [NamedValueInput], taken: &[&str]) -> Option<&'a str> {
    values
        .iter()
        .map(|value| value.name.as_str())
        .find(|name| taken.iter().any(|taken| taken.eq_ignore_ascii_case(name)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(name: &str, value: Option<&str>) -> NamedValueInput {
        NamedValueInput {
            name: name.into(),
            secret: false,
            value: value.map(str::to_string),
        }
    }

    #[test]
    fn secret_names_are_recognized() {
        for name in [
            "Authorization",
            "X-API-Key",
            "x-auth-token",
            "GITHUB_PERSONAL_ACCESS_TOKEN",
            "DB_PASSWORD",
            "Cookie",
        ] {
            assert!(looks_secret(name), "{name}");
        }

        for name in ["x-org", "LOG_LEVEL", "x-region"] {
            assert!(!looks_secret(name), "{name}");
        }
    }

    #[test]
    fn headers_are_lowercased_and_secrets_need_no_value() {
        let values = normalize_values(
            ValueKind::Header,
            vec![input("X-Org", Some(" acme ")), input("X-Api-Key", None)],
        )
        .unwrap();

        assert_eq!(values[0].name, "x-org");
        assert_eq!(values[0].value.as_deref(), Some("acme"));
        assert!(!values[0].secret);
        assert_eq!(values[1].name, "x-api-key");
        assert!(values[1].secret);
        assert_eq!(values[1].value, None);
    }

    #[test]
    fn a_secret_cannot_be_turned_into_plain_text() {
        let values = normalize_values(
            ValueKind::Header,
            vec![NamedValueInput {
                name: "Authorization".into(),
                secret: false,
                value: Some("Bearer x".into()),
            }],
        )
        .unwrap();

        assert!(values[0].secret);
    }

    #[test]
    fn bad_names_duplicates_and_reserved_headers_are_refused() {
        for values in [
            vec![input("Content-Type", Some("text/plain"))],
            vec![input("mcp-session-id", Some("x"))],
            vec![input("bad header", Some("x"))],
            vec![input("x-org", Some("a")), input("X-Org", Some("b"))],
            vec![input("x-org", None)],
            vec![input("x-org", Some("line\nbreak"))],
        ] {
            assert!(
                normalize_values(ValueKind::Header, values.clone()).is_err(),
                "{values:?}"
            );
        }

        assert!(normalize_values(ValueKind::Env, vec![input("1ST", Some("x"))]).is_err());
        assert!(normalize_values(ValueKind::Env, vec![input("LOG_LEVEL", Some("debug"))]).is_ok());
    }

    #[test]
    fn overlap_with_the_authentication_header_is_found() {
        let values = vec![input("x-org", Some("a")), input("authorization", None)];

        assert_eq!(
            find_reserved_overlap(&values, &["Authorization"]),
            Some("authorization")
        );
        assert_eq!(find_reserved_overlap(&values, &["x-api-key"]), None);
        assert_eq!(last4("abcdef"), "cdef");
        assert_eq!(last4("ab"), "ab");
    }
}
