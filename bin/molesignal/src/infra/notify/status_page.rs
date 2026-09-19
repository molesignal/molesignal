// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Notify transport adapter for anonymous status-page subscribers.

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use lettre::message::Mailbox;
use reqwest::redirect::Policy;
use url::Url;

use super::EmailSender;
use crate::{
    domain::status_page::{
        StatusPageNotificationMessage, StatusPageNotificationSender, StatusPageSubscriberChannel,
    },
    shared::{
        Error, Result,
        http_trace::{self, HttpTarget},
    },
};

pub struct StatusPageNotifyAdapter {
    email: Option<Arc<EmailSender>>,
}

impl StatusPageNotifyAdapter {
    pub fn new(email: Option<Arc<EmailSender>>) -> Self {
        Self { email }
    }
}

#[async_trait]
impl StatusPageNotificationSender for StatusPageNotifyAdapter {
    async fn validate_target(
        &self,
        channel: StatusPageSubscriberChannel,
        target: &str,
    ) -> Result<()> {
        match channel {
            StatusPageSubscriberChannel::Email => {
                target
                    .trim()
                    .parse::<Mailbox>()
                    .map_err(|_| Error::invalid("subscription email address is invalid"))?;
                Ok(())
            }
            StatusPageSubscriberChannel::Webhook => {
                resolve_public_webhook(target).await?;
                Ok(())
            }
        }
    }

    async fn send(
        &self,
        channel: StatusPageSubscriberChannel,
        target: &str,
        message: &StatusPageNotificationMessage,
    ) -> Result<()> {
        match channel {
            StatusPageSubscriberChannel::Email => {
                let sender = self.email.as_ref().ok_or_else(|| {
                    Error::unavailable("status-page email delivery is not configured")
                })?;
                sender
                    .send_text(
                        &[target.trim().to_ascii_lowercase()],
                        &message.title,
                        &message.text,
                    )
                    .await
            }
            StatusPageSubscriberChannel::Webhook => send_webhook(target, message).await,
        }
    }
}

async fn send_webhook(target: &str, message: &StatusPageNotificationMessage) -> Result<()> {
    let (url, host, address) = resolve_public_webhook(target).await?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(Policy::none())
        .resolve(&host, address)
        .build()
        .map_err(|_| Error::internal("status-page webhook client initialization failed"))?;
    let response = http_trace::send(
        &client,
        client
            .post(url)
            .header("user-agent", "MoleSignal-Status-Page/1.0")
            .json(&message.payload),
        HttpTarget::ThirdParty,
    )
    .await
    .map_err(|_| Error::unavailable("status-page webhook request failed"))?;
    if !response.status().is_success() {
        return Err(Error::unavailable(format!(
            "status-page webhook returned HTTP {}",
            response.status().as_u16()
        )));
    }
    Ok(())
}

async fn resolve_public_webhook(target: &str) -> Result<(Url, String, SocketAddr)> {
    let url = Url::parse(target.trim())
        .map_err(|_| Error::invalid("webhook URL must be an absolute HTTPS URL"))?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return Err(Error::invalid(
            "webhook URL must use HTTPS and cannot contain credentials",
        ));
    }
    let host = url
        .host_str()
        .filter(|host| !host.is_empty())
        .ok_or_else(|| Error::invalid("webhook URL must include a host"))?
        .to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") {
        return Err(Error::invalid("webhook URL must use a public host"));
    }
    let port = url.port_or_known_default().unwrap_or(443);
    let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|_| Error::invalid("webhook host could not be resolved"))?
        .filter(|address| is_public_ip(address.ip()))
        .collect();
    let address = addresses
        .into_iter()
        .next()
        .ok_or_else(|| Error::invalid("webhook host does not resolve to a public address"))?;
    Ok((url, host, address))
}

fn is_public_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_public_ipv4(address),
        IpAddr::V6(address) => is_public_ipv6(address),
    }
}

fn is_public_ipv4(address: Ipv4Addr) -> bool {
    let [a, b, c, _] = address.octets();
    !(address.is_private()
        || address.is_loopback()
        || address.is_link_local()
        || address.is_broadcast()
        || address.is_documentation()
        || address.is_unspecified()
        || address.is_multicast()
        || a == 0
        || (a == 100 && (64..=127).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 88 && c == 99)
        || (a == 198 && (18..=19).contains(&b))
        || a >= 240)
}

fn is_public_ipv6(address: Ipv6Addr) -> bool {
    let segments = address.segments();
    if let Some(mapped) = address.to_ipv4_mapped() {
        return is_public_ipv4(mapped);
    }
    !(address.is_unspecified()
        || address.is_loopback()
        || address.is_multicast()
        || segments[0] & 0xfe00 == 0xfc00
        || segments[0] & 0xffc0 == 0xfe80
        || segments[0] & 0xffc0 == 0xfec0
        || (segments[0] == 0x2001 && segments[1] == 0x0db8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_ip_filter_blocks_internal_and_documentation_ranges() {
        assert!(!is_public_ip("127.0.0.1".parse().unwrap()));
        assert!(!is_public_ip("10.0.0.2".parse().unwrap()));
        assert!(!is_public_ip("169.254.169.254".parse().unwrap()));
        assert!(!is_public_ip("::ffff:127.0.0.1".parse().unwrap()));
        assert!(!is_public_ip("2001:db8::1".parse().unwrap()));
        assert!(is_public_ip("1.1.1.1".parse().unwrap()));
        assert!(is_public_ip("2606:4700:4700::1111".parse().unwrap()));
    }
}
