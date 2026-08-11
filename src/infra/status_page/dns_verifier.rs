// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{collections::HashSet, time::Duration};

use async_trait::async_trait;
use serde::Deserialize;
use url::Url;

use crate::{
    domain::status_page::{StatusPageDomainCheck, StatusPageDomainVerifier},
    shared::{Error, Result},
};

const DNS_GOOGLE_RESOLVE: &str = "https://dns.google/resolve";

pub struct DnsStatusPageDomainVerifier {
    client: reqwest::Client,
    routing_target: String,
}

impl DnsStatusPageDomainVerifier {
    pub fn new(external_url: &str) -> Result<Self> {
        let routing_target = Url::parse(external_url.trim().trim_end_matches('/'))
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .ok_or_else(|| {
                Error::unavailable(
                    "http.external_url must include a hostname for custom-domain routing",
                )
            })?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| Error::internal("DNS verification client could not be built"))?;
        Ok(Self {
            client,
            routing_target: routing_target.trim_end_matches('.').to_ascii_lowercase(),
        })
    }

    async fn query(&self, name: &str, record_type: &str) -> Result<Vec<DnsAnswer>> {
        let mut url = Url::parse(DNS_GOOGLE_RESOLVE)
            .map_err(|_| Error::internal("DNS verification provider URL is invalid"))?;
        url.query_pairs_mut()
            .append_pair("name", name)
            .append_pair("type", record_type)
            .append_pair("cd", "false");
        let response = self
            .client
            .get(url)
            .header(reqwest::header::ACCEPT, "application/dns-json")
            .send()
            .await
            .map_err(|_| Error::unavailable("DNS verification provider is unavailable"))?;
        if !response.status().is_success() {
            return Err(Error::unavailable(
                "DNS verification provider returned an error",
            ));
        }
        let response: DnsResponse = response
            .json()
            .await
            .map_err(|_| Error::unavailable("DNS verification response is invalid"))?;
        if response.status != 0 && response.status != 3 {
            return Err(Error::unavailable(format!(
                "DNS lookup failed with resolver status {}",
                response.status
            )));
        }
        Ok(response.answer.unwrap_or_default())
    }

    async fn routing_matches(&self, hostname: &str) -> Result<bool> {
        let (custom_cname, custom_a, custom_aaaa, target_a, target_aaaa) = tokio::try_join!(
            self.query(hostname, "CNAME"),
            self.query(hostname, "A"),
            self.query(hostname, "AAAA"),
            self.query(&self.routing_target, "A"),
            self.query(&self.routing_target, "AAAA"),
        )?;
        let cname_matches = custom_cname.iter().any(|answer| {
            answer.record_type == 5
                && answer
                    .data
                    .trim_end_matches('.')
                    .eq_ignore_ascii_case(&self.routing_target)
        });
        if cname_matches {
            return Ok(true);
        }
        let custom_addresses = address_set(custom_a.into_iter().chain(custom_aaaa));
        let target_addresses = address_set(target_a.into_iter().chain(target_aaaa));
        Ok(!custom_addresses.is_empty()
            && custom_addresses
                .intersection(&target_addresses)
                .next()
                .is_some())
    }
}

#[async_trait]
impl StatusPageDomainVerifier for DnsStatusPageDomainVerifier {
    async fn verify(
        &self,
        hostname: &str,
        verification_token: &str,
    ) -> Result<StatusPageDomainCheck> {
        let txt_name = format!("_molesignal-verification.{hostname}");
        let txt = self.query(&txt_name, "TXT").await?;
        let ownership_verified = txt.iter().any(|answer| {
            answer.record_type == 16 && normalize_txt(&answer.data) == verification_token
        });
        let routing_valid = self.routing_matches(hostname).await?;
        let error = match (ownership_verified, routing_valid) {
            (false, false) => Some("DNS TXT ownership and routing records are not ready".into()),
            (false, true) => Some("DNS TXT ownership record is not ready".into()),
            (true, false) => Some("DNS routing record does not target MoleSignal".into()),
            (true, true) => None,
        };
        Ok(StatusPageDomainCheck {
            ownership_verified,
            routing_valid,
            error,
        })
    }

    fn routing_target(&self) -> &str {
        &self.routing_target
    }
}

#[derive(Debug, Deserialize)]
struct DnsResponse {
    #[serde(rename = "Status")]
    status: i32,
    #[serde(rename = "Answer")]
    answer: Option<Vec<DnsAnswer>>,
}

#[derive(Debug, Deserialize)]
struct DnsAnswer {
    #[serde(rename = "type")]
    record_type: u16,
    data: String,
}

fn address_set(answers: impl Iterator<Item = DnsAnswer>) -> HashSet<String> {
    answers
        .filter(|answer| matches!(answer.record_type, 1 | 28))
        .map(|answer| answer.data.to_ascii_lowercase())
        .collect()
}

fn normalize_txt(value: &str) -> String {
    let value = value.trim();
    if !value.contains('"') {
        return value.to_string();
    }
    value
        .split('"')
        .enumerate()
        .filter_map(|(index, part)| (index % 2 == 1).then_some(part))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn txt_chunks_are_joined_without_quotes() {
        assert_eq!(normalize_txt(r#""msv_ab" "cd""#), "msv_abcd");
        assert_eq!(normalize_txt("msv_plain"), "msv_plain");
    }

    #[test]
    fn verifier_uses_external_url_hostname_as_routing_target() {
        let verifier = DnsStatusPageDomainVerifier::new("https://status.example.com/base").unwrap();
        assert_eq!(verifier.routing_target(), "status.example.com");
    }
}
