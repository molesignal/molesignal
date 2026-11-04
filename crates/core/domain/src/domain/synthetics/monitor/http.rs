// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::fmt;

use serde::{Deserialize, Serialize};

use super::{Extraction, MonitorAssertion, ValueSource};
use crate::shared::ids::Id;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeaderValue {
    pub name: String,
    pub value: ValueSource,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HttpStep {
    pub id: Id,
    pub name: String,
    pub method: String,
    pub url: ValueSource,
    #[serde(default)]
    pub headers: Vec<HeaderValue>,
    #[serde(default)]
    pub query: Vec<HeaderValue>,
    pub body: Option<ValueSource>,
    #[serde(default)]
    pub extractions: Vec<Extraction>,
    #[serde(default)]
    pub assertions: Vec<MonitorAssertion>,
}

/// The HTTP protocol a check speaks: what TLS negotiates, or one of them only. Over plain HTTP,
/// HTTP/2 means prior knowledge (h2c).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HttpVersion {
    #[default]
    Auto,
    Http1,
    Http2,
}

/// The certificate a check shows a server that asks for one (mutual TLS), and its private key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientCertificate {
    /// The certificate chain in PEM, stored and shown as written: none of it is secret.
    pub certificate: String,
    pub private_key: ClientKey,
}

/// The private key of a client certificate. It is sealed when a revision is stored, and never
/// read back out: a revision holds it as `Kept`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ClientKey {
    /// A key in PEM, as uploaded or pasted.
    New { pem: String },
    /// The key a revision holds. A revision shows its own key this way, and a revision saved
    /// with another's keeps that key.
    Kept { revision_id: Id },
}

impl fmt::Debug for ClientKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::New { .. } => formatter.write_str("New { pem: <redacted> }"),
            Self::Kept { revision_id } => formatter
                .debug_struct("Kept")
                .field("revision_id", revision_id)
                .finish(),
        }
    }
}

/// A host name the check reaches at addresses of its own choosing, as a hosts file would: the
/// request still names the host, in its Host header and in TLS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostBinding {
    pub name: String,
    pub addresses: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpJourneySpec {
    pub steps: Vec<HttpStep>,
    pub follow_redirects: bool,
    pub max_redirects: u8,
    pub verify_tls: bool,
    #[serde(default)]
    pub http_version: HttpVersion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_certificate: Option<ClientCertificate>,
    /// The DNS server host names are looked up on, instead of the Agent's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dns_server: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hosts: Vec<HostBinding>,
}
