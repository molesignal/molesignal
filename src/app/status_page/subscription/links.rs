// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Public page links and subscriber-facing notification copy.

use serde_json::{Value, json};
use url::Url;

use crate::{
    domain::status_page::{
        StatusPage, StatusPageNotificationDelivery, StatusPageNotificationMessage,
    },
    shared::{Error, Result},
};

pub(super) fn platform_page_url(external_url: &str, slug: &str) -> Result<String> {
    let base = http_url(
        external_url,
        "http.external_url is required for status-page subscriptions",
    )?;
    base.join(&format!("/status/{slug}"))
        .map(|url| url.to_string())
        .map_err(|_| Error::internal("status-page URL could not be built"))
}

pub(super) fn custom_page_url(hostname: &str) -> Result<String> {
    http_url(
        &format!("https://{hostname}/"),
        "active status-page custom domain is invalid",
    )
    .map(|url| url.to_string())
}

pub(super) fn confirmation_message(
    page: &StatusPage,
    page_url: &str,
    token: &str,
) -> Result<StatusPageNotificationMessage> {
    let confirm_url = subscription_action_url(page_url, "confirm", token)?;
    let unsubscribe_url = subscription_action_url(page_url, "unsubscribe", token)?;
    Ok(StatusPageNotificationMessage {
        title: format!("Confirm updates from {}", page.name),
        text: format!(
            "Confirm your subscription to {name}:\n{confirm}\n\nIf you did not request this, no action is required. You can unsubscribe with:\n{unsubscribe}",
            name = page.name,
            confirm = confirm_url,
            unsubscribe = unsubscribe_url,
        ),
        payload: json!({
            "type": "status_page_subscription_confirmation",
            "page_name": page.name,
            "page_slug": page.slug,
            "confirm_url": confirm_url,
            "unsubscribe_url": unsubscribe_url,
            "token": token,
        }),
    })
}

pub(super) fn delivery_message(
    delivery: &StatusPageNotificationDelivery,
    page_url: &str,
) -> Result<StatusPageNotificationMessage> {
    let payload = &delivery.payload;
    let page_name = payload_text(payload, "page_name", "Status page")?;
    match payload.get("type").and_then(Value::as_str) {
        Some("component_status_changed") => {
            let component = payload_text(payload, "component_name", "component")?;
            let status = payload_text(payload, "status", "component status")?;
            Ok(StatusPageNotificationMessage {
                title: format!("{page_name}: {component} is {status}"),
                text: format!("{component} changed to {status}.\n\n{page_url}"),
                payload: delivery.payload.clone(),
            })
        }
        Some("incident_update") => {
            let title = payload_text(payload, "title", "incident title")?;
            let status = payload_text(payload, "status", "incident status")?;
            let message = payload_text(payload, "message", "incident message")?;
            Ok(StatusPageNotificationMessage {
                title: format!("{page_name}: {title} · {status}"),
                text: format!("{title}\n{status}\n\n{message}\n\n{page_url}"),
                payload: delivery.payload.clone(),
            })
        }
        _ => Err(Error::internal("unknown status-page notification payload")),
    }
}

fn subscription_action_url(page_url: &str, action: &str, token: &str) -> Result<String> {
    let mut url = http_url(page_url, "status-page subscription URL is invalid")?;
    let fragment = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("subscription_action", action)
        .append_pair("subscription_token", token)
        .finish();
    url.set_fragment(Some(&fragment));
    Ok(url.to_string())
}

fn http_url(value: &str, error: &'static str) -> Result<Url> {
    let url = Url::parse(value.trim()).map_err(|_| Error::unavailable(error))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(Error::unavailable(error));
    }
    Ok(url)
}

fn payload_text<'a>(payload: &'a Value, key: &str, label: &str) -> Result<&'a str> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| Error::internal(format!("status-page notification missing {label}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscription_links_are_action_bound_and_url_encoded() {
        let page_url = platform_page_url("https://molesignal.example/base", "acme-cloud").unwrap();
        let url =
            subscription_action_url(&page_url, "confirm", "abc.DEF-12345678901234567890").unwrap();
        assert!(url.starts_with("https://molesignal.example/status/acme-cloud#"));
        assert!(url.contains("subscription_action=confirm"));
        assert!(url.contains("subscription_token=abc.DEF-"));
    }

    #[test]
    fn active_custom_domain_uses_the_site_root() {
        assert_eq!(
            custom_page_url("status.acme.example").unwrap(),
            "https://status.acme.example/"
        );
    }
}
