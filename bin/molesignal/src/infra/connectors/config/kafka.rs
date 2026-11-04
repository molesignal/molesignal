// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A Kafka destination: the brokers and topic, whether the connection is encrypted, and how the
//! client proves who it is (SASL PLAIN or SCRAM, or a client certificate over TLS).
//!
//! The settings become librdkafka properties in [`KafkaSettings::client_properties`]; certificates
//! and the key travel as PEM text, so nothing is written to disk.

use serde_json::Value;

use super::{auth_mode, flag, require, text, unused};
use crate::shared::{Error, Result};

const MODES: &[&str] = &[
    "none",
    "sasl_plain",
    "sasl_scram_sha_256",
    "sasl_scram_sha_512",
    "mtls",
];

const SASL_FIELDS: &[&str] = &["saslUsername", "saslPassword"];
const CLIENT_CERT_FIELDS: &[&str] = &["tlsClientCert", "tlsClientKey"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sasl {
    /// The librdkafka name of the mechanism.
    pub mechanism: &'static str,
    pub username: String,
    pub password: String,
}

/// A certificate the client presents, with its private key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientCertificate {
    pub certificate: String,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KafkaSettings {
    pub brokers: String,
    pub topic: String,
    /// The connection is encrypted with TLS.
    pub tls: bool,
    /// The CA that signed the brokers' certificates, when it is not one the system trusts.
    pub ca_certificate: Option<String>,
    pub client_certificate: Option<ClientCertificate>,
    pub sasl: Option<Sasl>,
}

impl KafkaSettings {
    pub fn parse(config: &Value) -> Result<Self> {
        require(config, "brokers", "kafka")?;
        require(config, "topic", "kafka")?;

        let tls = flag(config, "tls");
        let inferred = if text(config, "saslUsername").is_empty() {
            "none"
        } else {
            "sasl_plain"
        };
        let mode = auth_mode(config, MODES, inferred)?;

        let ca_certificate = pem(config, "tlsCaCert", "CERTIFICATE")?;

        if ca_certificate.is_some() && !tls {
            return Err(Error::invalid(
                "`tls_ca_cert` is only used over TLS: turn `tls` on",
            ));
        }

        let (sasl, client_certificate) = match mode {
            "none" => {
                unused(config, mode, &[SASL_FIELDS, CLIENT_CERT_FIELDS].concat())?;
                (None, None)
            }
            "mtls" => {
                unused(config, mode, SASL_FIELDS)?;

                if !tls {
                    return Err(Error::invalid(
                        "a client certificate is presented over TLS: turn `tls` on",
                    ));
                }

                (None, Some(client_certificate(config)?))
            }
            _ => {
                unused(config, mode, CLIENT_CERT_FIELDS)?;
                (Some(sasl(config, mode)?), None)
            }
        };

        Ok(Self {
            brokers: text(config, "brokers").to_string(),
            topic: text(config, "topic").to_string(),
            tls,
            ca_certificate,
            client_certificate,
            sasl,
        })
    }

    /// The librdkafka properties that make a client connect as these settings say.
    pub fn client_properties(&self) -> Vec<(&'static str, String)> {
        let protocol = match (self.tls, self.sasl.is_some()) {
            (false, false) => "plaintext",
            (true, false) => "ssl",
            (false, true) => "sasl_plaintext",
            (true, true) => "sasl_ssl",
        };

        let mut properties = vec![
            ("bootstrap.servers", self.brokers.clone()),
            ("security.protocol", protocol.to_string()),
        ];

        if let Some(sasl) = &self.sasl {
            properties.push(("sasl.mechanism", sasl.mechanism.to_string()));
            properties.push(("sasl.username", sasl.username.clone()));
            properties.push(("sasl.password", sasl.password.clone()));
        }

        if let Some(ca) = &self.ca_certificate {
            properties.push(("ssl.ca.pem", ca.clone()));
        }

        if let Some(client) = &self.client_certificate {
            properties.push(("ssl.certificate.pem", client.certificate.clone()));
            properties.push(("ssl.key.pem", client.key.clone()));
        }

        properties
    }
}

fn sasl(config: &Value, mode: &str) -> Result<Sasl> {
    let mechanism = match mode {
        "sasl_scram_sha_256" => "SCRAM-SHA-256",
        "sasl_scram_sha_512" => "SCRAM-SHA-512",
        _ => "PLAIN",
    };

    let (username, password) = (text(config, "saslUsername"), text(config, "saslPassword"));

    if username.is_empty() || password.is_empty() {
        return Err(Error::invalid(format!(
            "`auth` `{mode}` needs `sasl_username` and `sasl_password`"
        )));
    }

    Ok(Sasl {
        mechanism,
        username: username.to_string(),
        password: password.to_string(),
    })
}

fn client_certificate(config: &Value) -> Result<ClientCertificate> {
    let certificate = pem(config, "tlsClientCert", "CERTIFICATE")?;
    let key = pem(config, "tlsClientKey", "PRIVATE KEY")?;

    match (certificate, key) {
        (Some(certificate), Some(key)) => Ok(ClientCertificate { certificate, key }),
        _ => Err(Error::invalid(
            "a client certificate needs `tls_client_cert` and `tls_client_key`",
        )),
    }
}

/// PEM text under `key`, if any. `label` is what its `-----BEGIN` line must name, which tells a
/// certificate pasted where the key goes before a broker does.
fn pem(config: &Value, key: &str, label: &str) -> Result<Option<String>> {
    let value = text(config, key);

    if value.is_empty() {
        return Ok(None);
    }

    let names_label = value.lines().any(|line| {
        line.starts_with("-----BEGIN ") && line.trim_end().ends_with(&format!("{label}-----"))
    });

    if !names_label {
        return Err(Error::invalid(format!(
            "`{key}` must be PEM text whose `-----BEGIN` line names a {}",
            label.to_lowercase()
        )));
    }

    Ok(Some(value.to_string()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const CA: &str = "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----";
    const KEY: &str = "-----BEGIN PRIVATE KEY-----\nMIIE\n-----END PRIVATE KEY-----";
    const RSA_KEY: &str = "-----BEGIN RSA PRIVATE KEY-----\nMIIE\n-----END RSA PRIVATE KEY-----";

    fn kafka(extra: Value) -> Result<KafkaSettings> {
        let mut config = json!({ "brokers": "b-1:9093,b-2:9093", "topic": "logs" });
        config
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        KafkaSettings::parse(&config)
    }

    fn property<'a>(properties: &'a [(&str, String)], key: &str) -> Option<&'a str> {
        properties
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| value.as_str())
    }

    #[test]
    fn a_broker_that_asks_for_nothing_is_reached_in_the_clear() {
        let settings = kafka(json!({})).unwrap();
        let properties = settings.client_properties();

        assert_eq!(
            property(&properties, "security.protocol"),
            Some("plaintext")
        );
        assert_eq!(property(&properties, "sasl.mechanism"), None);
    }

    #[test]
    fn scram_over_tls_is_sasl_ssl_with_the_mechanism_named() {
        let settings = kafka(json!({
            "auth": "sasl_scram_sha_512",
            "tls": true,
            "saslUsername": "svc",
            "saslPassword": "pw",
        }))
        .unwrap();
        let properties = settings.client_properties();

        assert_eq!(property(&properties, "security.protocol"), Some("sasl_ssl"));
        assert_eq!(
            property(&properties, "sasl.mechanism"),
            Some("SCRAM-SHA-512")
        );
        assert_eq!(property(&properties, "sasl.username"), Some("svc"));
    }

    #[test]
    fn a_client_that_predates_auth_still_reads_as_plain() {
        let settings = kafka(json!({ "saslUsername": "svc", "saslPassword": "pw" })).unwrap();

        assert_eq!(settings.sasl.unwrap().mechanism, "PLAIN");
        assert!(kafka(json!({ "auth": "sasl_scram_sha_256", "saslUsername": "svc" })).is_err());
    }

    #[test]
    fn a_client_certificate_needs_tls_and_its_key() {
        let mtls = json!({
            "auth": "mtls",
            "tls": true,
            "tlsCaCert": CA,
            "tlsClientCert": CA,
            "tlsClientKey": RSA_KEY,
        });
        let settings = kafka(mtls).unwrap();
        let properties = settings.client_properties();

        assert_eq!(property(&properties, "security.protocol"), Some("ssl"));
        assert_eq!(property(&properties, "ssl.ca.pem"), Some(CA));
        assert_eq!(property(&properties, "ssl.key.pem"), Some(RSA_KEY));

        assert!(
            kafka(json!({ "auth": "mtls", "tlsClientCert": CA, "tlsClientKey": KEY })).is_err()
        );
        assert!(kafka(json!({ "auth": "mtls", "tls": true, "tlsClientCert": CA })).is_err());
    }

    #[test]
    fn pem_text_is_told_apart_from_its_neighbour() {
        // The key pasted where the certificate goes, and the other way round.
        assert!(
            kafka(
                json!({ "auth": "mtls", "tls": true, "tlsClientCert": KEY, "tlsClientKey": KEY })
            )
            .is_err()
        );
        assert!(
            kafka(json!({ "auth": "mtls", "tls": true, "tlsClientCert": CA, "tlsClientKey": CA }))
                .is_err()
        );
        assert!(kafka(json!({ "tls": true, "tlsCaCert": "not pem" })).is_err());
        assert!(kafka(json!({ "tlsCaCert": CA })).is_err());
    }

    #[test]
    fn credentials_the_mode_does_not_use_are_refused() {
        assert!(
            kafka(json!({ "auth": "none", "saslUsername": "svc", "saslPassword": "pw" })).is_err()
        );
        assert!(
            kafka(json!({
                "auth": "sasl_plain",
                "saslUsername": "svc",
                "saslPassword": "pw",
                "tlsClientKey": KEY,
            }))
            .is_err()
        );
    }
}
