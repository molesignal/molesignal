// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! An HTTP destination: the address a pipeline's events are sent to, how they are written, and
//! how the request proves who sent it.
//!
//! The ways of authenticating are the ones receivers ask for: a bearer token, a user name and
//! password (Basic), an API key in a header of the receiver's choosing, or an HMAC-SHA256
//! signature of the body that the receiver checks with a secret both sides hold.

use std::net::IpAddr;

use reqwest::header::{HeaderName, HeaderValue};
use serde_json::Value;
use url::Url;

use super::{auth_mode, text, unused};
use crate::shared::{Error, Result};

const MODES: &[&str] = &["none", "bearer", "basic", "header", "hmac"];

const FIELDS: &[&str] = &[
    "bearerToken",
    "basicUsername",
    "basicPassword",
    "headerName",
    "headerValue",
    "hmacSecret",
];

/// A shared secret shorter than this is too easy to guess to sign with.
const MIN_HMAC_SECRET_LEN: usize = 16;

/// Headers the delivery sets itself, so a credential cannot be put in one of them.
const RESERVED_HEADERS: &[&str] = &[
    "host",
    "content-length",
    "content-type",
    "content-encoding",
    "transfer-encoding",
    "connection",
    "x-molesignal-timestamp",
    "x-molesignal-sign",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Post,
    Put,
}

/// How a batch is written in the request body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyFormat {
    /// One JSON object per line (`application/x-ndjson`).
    Ndjson,
    /// One JSON array of the events (`application/json`).
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpAuth {
    None,
    Bearer(String),
    Basic {
        username: String,
        password: String,
    },
    Header {
        name: String,
        value: String,
    },
    /// The key both sides sign and check the body with.
    Hmac(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpSettings {
    pub url: Url,
    pub method: HttpMethod,
    pub format: BodyFormat,
    pub auth: HttpAuth,
}

impl HttpSettings {
    pub fn parse(config: &Value) -> Result<Self> {
        let url = url(config)?;

        let method = match text(config, "method").to_ascii_lowercase().as_str() {
            "" | "post" => HttpMethod::Post,
            "put" => HttpMethod::Put,
            other => {
                return Err(Error::invalid(format!(
                    "`method` must be post or put, not `{other}`"
                )));
            }
        };

        let format = match text(config, "format") {
            "" | "ndjson" => BodyFormat::Ndjson,
            "json" => BodyFormat::Json,
            other => {
                return Err(Error::invalid(format!(
                    "`format` must be ndjson or json, not `{other}`"
                )));
            }
        };

        let mode = auth_mode(config, MODES, "none")?;
        let used: &[&str] = match mode {
            "bearer" => &["bearerToken"],
            "basic" => &["basicUsername", "basicPassword"],
            "header" => &["headerName", "headerValue"],
            "hmac" => &["hmacSecret"],
            _ => &[],
        };

        let others: Vec<&str> = FIELDS
            .iter()
            .copied()
            .filter(|field| !used.contains(field))
            .collect();

        unused(config, mode, &others)?;

        if let Some(missing) = used.iter().find(|field| text(config, field).is_empty()) {
            return Err(Error::invalid(format!("`auth` `{mode}` needs `{missing}`")));
        }

        let value = |key: &str| text(config, key).to_string();

        let auth = match mode {
            "bearer" => HttpAuth::Bearer(header_safe("bearerToken", &value("bearerToken"))?),
            "basic" => HttpAuth::Basic {
                username: value("basicUsername"),
                password: value("basicPassword"),
            },
            "header" => header(&value("headerName"), &value("headerValue"))?,
            "hmac" => hmac(&value("hmacSecret"))?,
            _ => HttpAuth::None,
        };

        Ok(Self {
            url,
            method,
            format,
            auth,
        })
    }

    /// Where the destination points, without what its address may carry past the host.
    pub fn origin(&self) -> String {
        self.url.origin().ascii_serialization()
    }
}

fn url(config: &Value) -> Result<Url> {
    let address = text(config, "url");

    if address.is_empty() {
        return Err(Error::invalid("http connector requires `url`"));
    }

    let url = Url::parse(address)
        .map_err(|_| Error::invalid("`url` must be an absolute http:// or https:// URL"))?;

    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(Error::invalid(
            "`url` must be an absolute http:// or https:// URL",
        ));
    }

    if !url.username().is_empty() || url.password().is_some() {
        return Err(Error::invalid(
            "`url` must not carry a user name or password: use `auth` `basic`",
        ));
    }

    let literal = match url.host() {
        Some(url::Host::Ipv4(ip)) => Some(IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => Some(IpAddr::V6(ip)),
        _ => None,
    };

    if let Some(ip) = literal.filter(|ip| is_metadata_address(*ip)) {
        return Err(Error::invalid(format!(
            "`url` points at `{ip}`, where cloud metadata services answer"
        )));
    }

    Ok(url)
}

/// Where cloud instance metadata services answer, which no receiver does: the link-local ranges
/// (169.254.0.0/16, fe80::/10), AWS's IPv6 endpoint `fd00:ec2::254`, and Alibaba Cloud's
/// `100.100.100.200`.
pub fn is_metadata_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_link_local() || ip.octets() == [100, 100, 100, 200],
        IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
            Some(mapped) => is_metadata_address(IpAddr::V4(mapped)),
            None => {
                (ip.segments()[0] & 0xffc0) == 0xfe80
                    || ip.segments() == [0xfd00, 0x0ec2, 0, 0, 0, 0, 0, 0x0254]
            }
        },
    }
}

fn header(name: &str, value: &str) -> Result<HttpAuth> {
    let parsed = HeaderName::from_bytes(name.as_bytes())
        .map_err(|_| Error::invalid(format!("`header_name` `{name}` is not a header name")))?;

    if RESERVED_HEADERS.contains(&parsed.as_str()) {
        return Err(Error::invalid(format!(
            "`header_name` `{name}` is set by the delivery itself"
        )));
    }

    Ok(HttpAuth::Header {
        name: parsed.as_str().to_string(),
        value: header_safe("headerValue", value)?,
    })
}

fn hmac(secret: &str) -> Result<HttpAuth> {
    if secret.len() < MIN_HMAC_SECRET_LEN {
        return Err(Error::invalid(format!(
            "`hmac_secret` must be at least {MIN_HMAC_SECRET_LEN} characters"
        )));
    }

    Ok(HttpAuth::Hmac(secret.to_string()))
}

/// A value that can travel in a header: no line breaks or other control characters.
fn header_safe(key: &str, value: &str) -> Result<String> {
    HeaderValue::from_str(value)
        .map_err(|_| Error::invalid(format!("`{key}` cannot be sent in a header")))?;

    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn http(extra: Value) -> Result<HttpSettings> {
        let mut config = json!({ "url": "https://in.example.com/v1/logs" });
        config
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        HttpSettings::parse(&config)
    }

    #[test]
    fn a_bare_address_posts_ndjson_without_credentials() {
        let settings = http(json!({})).unwrap();

        assert_eq!(settings.method, HttpMethod::Post);
        assert_eq!(settings.format, BodyFormat::Ndjson);
        assert_eq!(settings.auth, HttpAuth::None);
        assert_eq!(settings.origin(), "https://in.example.com");
    }

    #[test]
    fn each_way_of_authenticating_takes_its_own_fields() {
        assert_eq!(
            http(json!({ "auth": "bearer", "bearerToken": "t0k" }))
                .unwrap()
                .auth,
            HttpAuth::Bearer("t0k".into())
        );
        assert_eq!(
            http(json!({ "auth": "basic", "basicUsername": "u", "basicPassword": "p" }))
                .unwrap()
                .auth,
            HttpAuth::Basic {
                username: "u".into(),
                password: "p".into()
            }
        );
        assert_eq!(
            http(json!({ "auth": "header", "headerName": "DD-API-KEY", "headerValue": "k" }))
                .unwrap()
                .auth,
            HttpAuth::Header {
                name: "dd-api-key".into(),
                value: "k".into()
            }
        );
        assert!(http(json!({ "auth": "basic", "basicUsername": "u" })).is_err());
        assert!(
            http(json!({ "auth": "bearer", "basicPassword": "p", "bearerToken": "t" })).is_err()
        );
    }

    #[test]
    fn a_signing_secret_is_long_enough_to_sign_with() {
        assert!(http(json!({ "auth": "hmac", "hmacSecret": "0123456789abcdef" })).is_ok());
        assert!(http(json!({ "auth": "hmac", "hmacSecret": "short" })).is_err());
    }

    #[test]
    fn a_credential_header_cannot_stand_in_for_one_the_delivery_sets() {
        for name in ["Content-Type", "X-MoleSignal-Sign", "bad header"] {
            assert!(
                http(json!({ "auth": "header", "headerName": name, "headerValue": "v" })).is_err(),
                "{name}"
            );
        }

        assert!(
            http(json!({ "auth": "header", "headerName": "X-Api-Key", "headerValue": "a\nb" }))
                .is_err()
        );
    }

    #[test]
    fn the_address_is_a_plain_http_url() {
        let with = |url: &str| HttpSettings::parse(&json!({ "url": url }));

        assert!(with("http://collector.internal:8080/ingest").is_ok());
        assert!(with("ftp://collector.example").is_err());
        assert!(with("collector.example/logs").is_err());
        assert!(with("https://user:pass@collector.example").is_err());
        assert!(with("http://169.254.169.254/latest/meta-data/").is_err());
        assert!(with("http://[fe80::1]/").is_err());
        assert!(with("http://[fd00:ec2::254]/latest/meta-data/").is_err());
        assert!(with("http://127.0.0.1:8686/").is_ok());
    }

    #[test]
    fn method_and_format_are_read_from_their_names() {
        let settings = http(json!({ "method": "PUT", "format": "json" })).unwrap();

        assert_eq!(settings.method, HttpMethod::Put);
        assert_eq!(settings.format, BodyFormat::Json);
        assert!(http(json!({ "method": "get" })).is_err());
        assert!(http(json!({ "format": "csv" })).is_err());
    }

    #[test]
    fn metadata_addresses_of_both_families_are_known() {
        for ip in [
            "169.254.169.254",
            "169.254.170.2",
            "100.100.100.200",
            "fe80::1",
            "fd00:ec2::254",
            "::ffff:169.254.169.254",
        ] {
            assert!(is_metadata_address(ip.parse().unwrap()), "{ip}");
        }

        for ip in [
            "10.0.0.1",
            "100.100.100.201",
            "fd00:ec2::253",
            "2606:4700::1",
        ] {
            assert!(!is_metadata_address(ip.parse().unwrap()), "{ip}");
        }
    }
}
