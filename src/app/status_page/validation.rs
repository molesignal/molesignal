// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashSet;

use url::Url;

use super::{
    MAX_STATUS_PAGE_HISTORY_DAYS, StatusPageComponentInput, StatusPageIncidentInput,
    StatusPageInput,
};
use crate::{
    domain::status_page::{
        IncidentImpact, PublicIncidentStatus, StatusPageIncidentKind, StatusPagePublicationState,
    },
    shared::{Error, Result},
};

const MAX_NAME_LEN: usize = 120;
const MAX_TITLE_LEN: usize = 200;
const MAX_MESSAGE_LEN: usize = 4_000;
const SUPPORTED_LANGUAGES: [&str; 2] = ["en-us", "zh-cn"];

pub(super) fn normalize_page_input(mut input: StatusPageInput) -> Result<StatusPageInput> {
    input.name = input.name.trim().to_string();
    if input.name.is_empty()
        || input.name.chars().count() > MAX_NAME_LEN
        || input.name.chars().any(char::is_control)
    {
        return Err(Error::invalid("name must contain 1 to 120 characters"));
    }
    input.slug = input.slug.trim().to_ascii_lowercase();
    validate_slug(&input.slug)?;
    input.logo_url = normalize_logo_url(input.logo_url)?;
    input.brand_color = normalize_brand_color(&input.brand_color)?;
    input.custom_domain = normalize_custom_domain(input.custom_domain)?;
    input.timezone = input.timezone.trim().to_string();
    input
        .timezone
        .parse::<chrono_tz::Tz>()
        .map_err(|_| Error::invalid("timezone must be a valid IANA timezone"))?;
    input.language = input.language.trim().to_ascii_lowercase();
    if !SUPPORTED_LANGUAGES.contains(&input.language.as_str()) {
        return Err(Error::invalid("language must be en-us or zh-cn"));
    }
    input.languages = normalize_languages(input.languages, &input.language)?;
    if !(1..=MAX_STATUS_PAGE_HISTORY_DAYS).contains(&input.history_days) {
        return Err(Error::invalid("history_days must be between 1 and 365"));
    }
    if !matches!(input.delivery_retention_days, 30 | 60 | 90 | 180 | 365) {
        return Err(Error::invalid(
            "delivery_retention_days must be 30, 60, 90, 180, or 365",
        ));
    }
    if !matches!(input.private_session_days, 1 | 7 | 30) {
        return Err(Error::invalid("private_session_days must be 1, 7, or 30"));
    }
    Ok(input)
}

fn normalize_languages(languages: Vec<String>, default_language: &str) -> Result<Vec<String>> {
    if languages.is_empty() {
        return Ok(vec![default_language.to_string()]);
    }

    let mut normalized = Vec::with_capacity(languages.len().min(SUPPORTED_LANGUAGES.len()));
    let mut seen = HashSet::new();
    for language in languages {
        let language = language.trim().to_ascii_lowercase();
        if !SUPPORTED_LANGUAGES.contains(&language.as_str()) {
            return Err(Error::invalid("languages may only contain en-us or zh-cn"));
        }
        if seen.insert(language.clone()) {
            normalized.push(language);
        }
    }
    let Some(default_position) = normalized
        .iter()
        .position(|language| language == default_language)
    else {
        return Err(Error::invalid(
            "default language must be included in languages",
        ));
    };
    if default_position > 0 {
        let default_language = normalized.remove(default_position);
        normalized.insert(0, default_language);
    }
    Ok(normalized)
}

fn validate_slug(slug: &str) -> Result<()> {
    if !(3..=64).contains(&slug.len())
        || slug.starts_with('-')
        || slug.ends_with('-')
        || slug.contains("--")
        || !slug
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(Error::invalid(
            "slug must be 3 to 64 lowercase letters, numbers, or single hyphens",
        ));
    }
    Ok(())
}

pub(super) fn normalize_logo_url(value: Option<String>) -> Result<Option<String>> {
    let Some(value) = non_empty(value) else {
        return Ok(None);
    };
    if value.len() > 2_048 {
        return Err(Error::invalid("logo_url is too long"));
    }
    if value.starts_with('/')
        && !value.starts_with("//")
        && !value.split('/').any(|part| part == "..")
    {
        return Ok(Some(value));
    }
    let parsed =
        Url::parse(&value).map_err(|_| Error::invalid("logo_url must be an HTTP(S) URL"))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(Error::invalid(
            "logo_url must be an HTTP(S) URL without credentials",
        ));
    }
    Ok(Some(value))
}

fn normalize_brand_color(value: &str) -> Result<String> {
    let value = value.trim().to_ascii_uppercase();
    if value.len() != 7
        || !value.starts_with('#')
        || !value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(Error::invalid("brand_color must be a six-digit hex color"));
    }
    Ok(value)
}

pub(super) fn normalize_custom_domain(value: Option<String>) -> Result<Option<String>> {
    let Some(value) = non_empty(value) else {
        return Ok(None);
    };
    normalize_domain_lookup(&value).map(Some).ok_or_else(|| {
        Error::invalid("custom_domain must be a valid hostname without scheme or path")
    })
}

pub(super) fn normalize_domain_lookup(value: &str) -> Option<String> {
    let value = value.trim().trim_end_matches('.').to_ascii_lowercase();
    crate::domain_management::hostname_valid(&value)
        .ok()
        .map(|()| value)
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(super) fn validate_component_input(input: &StatusPageComponentInput) -> Result<()> {
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME_LEN || name.chars().any(char::is_control)
    {
        return Err(Error::invalid(
            "component name must contain 1 to 120 characters",
        ));
    }
    if input.description.chars().count() > 500 {
        return Err(Error::invalid(
            "component description cannot exceed 500 characters",
        ));
    }
    Ok(())
}

pub(super) fn validate_incident_input(input: &StatusPageIncidentInput) -> Result<()> {
    let title = input.title.trim();
    if title.chars().count() > MAX_TITLE_LEN || title.chars().any(char::is_control) {
        return Err(Error::invalid(
            "incident title cannot exceed 200 characters or contain controls",
        ));
    }
    if let Some(message) = input.message.as_deref()
        && !message.trim().is_empty()
    {
        validate_message(message)?;
    }
    if input.status.is_terminal_for(input.kind) {
        return Err(Error::invalid(
            "a new or draft event cannot already be terminal",
        ));
    }
    if input.publication_state == StatusPagePublicationState::Published {
        if title.is_empty() {
            return Err(Error::invalid(
                "incident title must contain 1 to 200 characters",
            ));
        }
        validate_message(input.message.as_deref().unwrap_or_default())?;
    }
    match (input.kind, input.impact) {
        (StatusPageIncidentKind::Incident, IncidentImpact::Maintenance)
        | (
            StatusPageIncidentKind::Maintenance,
            IncidentImpact::Minor | IncidentImpact::Major | IncidentImpact::Critical,
        ) => Err(Error::invalid("incident kind and impact are inconsistent")),
        _ => validate_kind_status(input.kind, input.status),
    }
}

fn validate_kind_status(kind: StatusPageIncidentKind, status: PublicIncidentStatus) -> Result<()> {
    let valid = match kind {
        StatusPageIncidentKind::Incident => matches!(
            status,
            PublicIncidentStatus::Investigating
                | PublicIncidentStatus::Identified
                | PublicIncidentStatus::InProgress
                | PublicIncidentStatus::Monitoring
                | PublicIncidentStatus::Resolved
        ),
        StatusPageIncidentKind::Maintenance => matches!(
            status,
            PublicIncidentStatus::Scheduled
                | PublicIncidentStatus::InProgress
                | PublicIncidentStatus::Completed
                | PublicIncidentStatus::Cancelled
        ),
    };
    if valid {
        Ok(())
    } else {
        Err(Error::invalid("event kind and status are inconsistent"))
    }
}

pub(super) fn validate_message(message: &str) -> Result<()> {
    let message = message.trim();
    if message.is_empty() || message.chars().count() > MAX_MESSAGE_LEN {
        return Err(Error::invalid(
            "update message must contain 1 to 4000 characters",
        ));
    }
    Ok(())
}

pub(super) fn validate_transition(
    kind: StatusPageIncidentKind,
    from: PublicIncidentStatus,
    to: PublicIncidentStatus,
) -> Result<()> {
    validate_kind_status(kind, to)?;
    let allowed = !from.is_terminal_for(kind)
        && (from == to
            || match kind {
                StatusPageIncidentKind::Incident => matches!(
                    (from, to),
                    (
                        PublicIncidentStatus::Investigating,
                        PublicIncidentStatus::Identified
                    ) | (
                        PublicIncidentStatus::Investigating,
                        PublicIncidentStatus::InProgress
                    ) | (
                        PublicIncidentStatus::Investigating,
                        PublicIncidentStatus::Monitoring
                    ) | (
                        PublicIncidentStatus::Investigating,
                        PublicIncidentStatus::Resolved
                    ) | (
                        PublicIncidentStatus::Identified,
                        PublicIncidentStatus::InProgress
                    ) | (
                        PublicIncidentStatus::Identified,
                        PublicIncidentStatus::Monitoring
                    ) | (
                        PublicIncidentStatus::Identified,
                        PublicIncidentStatus::Resolved
                    ) | (
                        PublicIncidentStatus::InProgress,
                        PublicIncidentStatus::Monitoring
                    ) | (
                        PublicIncidentStatus::InProgress,
                        PublicIncidentStatus::Resolved
                    ) | (
                        PublicIncidentStatus::Monitoring,
                        PublicIncidentStatus::InProgress
                    ) | (
                        PublicIncidentStatus::Monitoring,
                        PublicIncidentStatus::Resolved
                    )
                ),
                StatusPageIncidentKind::Maintenance => matches!(
                    (from, to),
                    (
                        PublicIncidentStatus::Scheduled,
                        PublicIncidentStatus::InProgress
                    ) | (
                        PublicIncidentStatus::Scheduled,
                        PublicIncidentStatus::Cancelled
                    ) | (
                        PublicIncidentStatus::InProgress,
                        PublicIncidentStatus::Completed
                    ) | (
                        PublicIncidentStatus::InProgress,
                        PublicIncidentStatus::Cancelled
                    )
                ),
            });
    if allowed {
        Ok(())
    } else {
        Err(Error::conflict(format!(
            "invalid public incident transition: {} -> {}",
            from.as_str(),
            to.as_str()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::status_page::StatusPageVisibility;

    fn page_input(language: &str, languages: &[&str]) -> StatusPageInput {
        StatusPageInput {
            name: "Acme Status".into(),
            slug: "acme-status".into(),
            logo_url: None,
            brand_color: "#4F46E5".into(),
            custom_domain: None,
            timezone: "UTC".into(),
            language: language.into(),
            languages: languages.iter().map(|value| (*value).into()).collect(),
            history_days: 90,
            delivery_retention_days: 90,
            private_session_days: 7,
            visibility: StatusPageVisibility::Public,
        }
    }

    #[test]
    fn omitted_languages_use_the_selected_default_language() {
        let normalized = normalize_page_input(page_input("en-us", &[])).expect("valid page");
        assert_eq!(normalized.languages, ["en-us"]);
    }

    #[test]
    fn languages_are_normalized_and_deduplicated() {
        let normalized = normalize_page_input(page_input("ZH-CN", &[" zh-cn ", "EN-US", "zh-cn"]))
            .expect("valid multilingual page");
        assert_eq!(normalized.language, "zh-cn");
        assert_eq!(normalized.languages, ["zh-cn", "en-us"]);
    }

    #[test]
    fn default_language_must_be_available_on_the_public_page() {
        let error = normalize_page_input(page_input("en-us", &["zh-cn"]))
            .expect_err("missing default language must fail");
        assert!(error.to_string().contains("default language"));
    }

    #[test]
    fn history_window_must_remain_bounded() {
        let mut input = page_input("en-us", &["en-us"]);
        input.history_days = 0;
        assert!(normalize_page_input(input.clone()).is_err());

        input.history_days = 366;
        assert!(normalize_page_input(input).is_err());
    }

    #[test]
    fn slug_rejects_ambiguous_or_unsafe_forms() {
        assert!(validate_slug("acme-cloud").is_ok());
        assert!(validate_slug("Acme").is_err());
        assert!(validate_slug("acme--cloud").is_err());
        assert!(validate_slug("../acme").is_err());
    }

    #[test]
    fn public_names_reject_header_control_characters() {
        let mut input = page_input("en-us", &["en-us"]);
        input.name = "Acme\r\nBcc: attacker@example.com".into();
        assert!(normalize_page_input(input).is_err());
    }

    #[test]
    fn custom_domains_and_request_hosts_share_one_canonical_form() {
        let mut input = page_input("en-us", &["en-us"]);
        input.custom_domain = Some(" Status.Acme.Example. ".into());
        let normalized = normalize_page_input(input).expect("valid custom domain");
        assert_eq!(
            normalized.custom_domain.as_deref(),
            Some("status.acme.example")
        );
        assert_eq!(
            normalize_domain_lookup("STATUS.ACME.EXAMPLE."),
            Some("status.acme.example".into())
        );
        assert_eq!(normalize_domain_lookup("localhost"), None);
        assert_eq!(normalize_domain_lookup("status_acme.example"), None);
        assert_eq!(normalize_domain_lookup("status.acme.example:443"), None);
        assert_eq!(normalize_domain_lookup("https://status.acme.example"), None);
    }

    #[test]
    fn resolved_incident_is_terminal_but_monitoring_can_regress() {
        assert!(
            validate_transition(
                StatusPageIncidentKind::Incident,
                PublicIncidentStatus::Monitoring,
                PublicIncidentStatus::InProgress
            )
            .is_ok()
        );
        assert!(
            validate_transition(
                StatusPageIncidentKind::Incident,
                PublicIncidentStatus::Resolved,
                PublicIncidentStatus::Investigating
            )
            .is_err()
        );
        assert!(
            validate_transition(
                StatusPageIncidentKind::Incident,
                PublicIncidentStatus::Resolved,
                PublicIncidentStatus::Resolved
            )
            .is_err()
        );
    }

    #[test]
    fn logo_urls_reject_credentials_and_traversal() {
        assert!(normalize_logo_url(Some("/brand/logo.svg".into())).is_ok());
        assert!(normalize_logo_url(Some("/brand/../secret".into())).is_err());
        assert!(normalize_logo_url(Some("https://user:pass@example.com/logo".into())).is_err());
    }
}
