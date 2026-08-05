// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! LDAP simple-bind authentication over LDAPS or StartTLS.

use std::{borrow::Cow, time::Duration};

use ldap3::{
    Ldap, LdapConnAsync, LdapConnSettings, Scope, SearchEntry, SearchOptions, ldap_escape,
};
use url::Url;

use crate::{
    domain::iam::SsoFieldMapping,
    shared::{Error, Result},
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const OPERATION_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct LdapConfig {
    pub url: String,
    pub start_tls: bool,
    pub bind_dn: String,
    pub bind_password: String,
    pub base_dn: String,
    pub user_filter: String,
    pub field_mapping: SsoFieldMapping,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LdapUser {
    pub subject: String,
    pub email: String,
    pub display_name: String,
    pub groups: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct LdapLoginFlow {
    cfg: LdapConfig,
}

impl LdapLoginFlow {
    pub fn new(cfg: LdapConfig) -> Result<Self> {
        validate_config(&cfg)?;
        Ok(Self { cfg })
    }

    /// Resolve the user's DN with the read-only bind, then re-bind as that DN.
    ///
    /// Empty passwords are rejected before contacting LDAP because several
    /// servers interpret an empty simple-bind password as anonymous access.
    pub async fn authenticate(&self, username: &str, password: &str) -> Result<LdapUser> {
        let username = username.trim();
        if username.is_empty()
            || username.len() > 320
            || password.is_empty()
            || password.len() > 1024
        {
            return Err(Error::unauthorized("invalid LDAP credentials"));
        }

        let mut ldap = self.connect().await?;
        if !self.cfg.bind_dn.trim().is_empty() {
            ldap.with_timeout(OPERATION_TIMEOUT)
                .simple_bind(&self.cfg.bind_dn, &self.cfg.bind_password)
                .await
                .map_err(|error| ldap_unavailable("service bind", error))?
                .success()
                .map_err(|error| ldap_unavailable("service bind", error))?;
        }

        let filter = render_user_filter(&self.cfg.user_filter, username);
        let mut attributes: Vec<&str> = Vec::with_capacity(4);
        for attribute in [
            self.cfg.field_mapping.subject.as_str(),
            self.cfg.field_mapping.email.as_str(),
            self.cfg.field_mapping.display_name.as_str(),
            self.cfg.field_mapping.groups.as_str(),
        ] {
            if !attribute.eq_ignore_ascii_case("dn")
                && !attributes
                    .iter()
                    .any(|existing| existing.eq_ignore_ascii_case(attribute))
            {
                attributes.push(attribute);
            }
        }
        let search = ldap
            .with_search_options(SearchOptions::new().sizelimit(2).timelimit(10))
            .with_timeout(OPERATION_TIMEOUT)
            .search(&self.cfg.base_dn, Scope::Subtree, &filter, attributes)
            .await
            .map_err(|error| ldap_unavailable("user search", error))?;
        let (entries, _) = search
            .success()
            .map_err(|error| ldap_unavailable("user search", error))?;
        if entries.len() != 1 {
            tracing::warn!(
                match_count = entries.len(),
                "LDAP user filter did not resolve exactly one entry"
            );
            let _ = ldap.unbind().await;
            return Err(Error::unauthorized("invalid LDAP credentials"));
        }

        let entry = SearchEntry::construct(
            entries
                .into_iter()
                .next()
                .expect("exactly one LDAP entry checked above"),
        );
        let user = user_from_entry(&entry, &self.cfg)?;

        let authenticated = ldap
            .with_timeout(OPERATION_TIMEOUT)
            .simple_bind(&entry.dn, password)
            .await
            .and_then(|result| result.success());
        if let Err(error) = authenticated {
            tracing::debug!(error = %error, "LDAP user bind rejected");
            let _ = ldap.unbind().await;
            return Err(Error::unauthorized("invalid LDAP credentials"));
        }
        let _ = ldap.unbind().await;
        Ok(user)
    }

    async fn connect(&self) -> Result<Ldap> {
        let settings = LdapConnSettings::new()
            .set_conn_timeout(CONNECT_TIMEOUT)
            .set_starttls(self.cfg.start_tls);
        let (connection, ldap) = LdapConnAsync::with_settings(settings, &self.cfg.url)
            .await
            .map_err(|error| ldap_unavailable("connect", error))?;
        tokio::spawn(async move {
            if let Err(error) = connection.drive().await {
                tracing::debug!(error = %error, "LDAP connection driver stopped");
            }
        });
        Ok(ldap)
    }
}

fn validate_config(cfg: &LdapConfig) -> Result<()> {
    let parsed =
        Url::parse(cfg.url.trim()).map_err(|_| Error::invalid("ldap.url must be a valid URL"))?;
    if parsed.host_str().is_none() || !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(Error::invalid(
            "ldap.url must contain a host and must not embed credentials",
        ));
    }
    match parsed.scheme() {
        "ldaps" if cfg.start_tls => {
            return Err(Error::invalid(
                "ldap.start_tls must be false when ldap.url uses ldaps://",
            ));
        }
        "ldaps" => {}
        "ldap" if cfg.start_tls => {}
        "ldap" => {
            return Err(Error::invalid(
                "ldap:// requires ldap.start_tls = true; plaintext LDAP is not allowed",
            ));
        }
        _ => {
            return Err(Error::invalid(
                "ldap.url scheme must be ldaps:// or ldap:// with StartTLS",
            ));
        }
    }
    if cfg.bind_dn.trim().is_empty() != cfg.bind_password.is_empty() {
        return Err(Error::invalid(
            "ldap.bind_dn and ldap.bind_password must either both be set or both be empty",
        ));
    }
    if cfg.base_dn.trim().is_empty() {
        return Err(Error::invalid("ldap.base_dn is required"));
    }
    if !cfg.user_filter.contains("{username}") || cfg.user_filter.len() > 2048 {
        return Err(Error::invalid(
            "ldap.user_filter must contain {username} and be at most 2048 characters",
        ));
    }
    for (field, value) in [
        ("subject", cfg.field_mapping.subject.as_str()),
        ("email", cfg.field_mapping.email.as_str()),
        ("display_name", cfg.field_mapping.display_name.as_str()),
        ("groups", cfg.field_mapping.groups.as_str()),
    ] {
        if value.trim().is_empty() || value.len() > 256 || value.chars().any(char::is_whitespace) {
            return Err(Error::invalid(format!(
                "ldap.field_mapping.{field} must be a non-empty LDAP attribute name"
            )));
        }
    }
    Ok(())
}

fn render_user_filter(template: &str, username: &str) -> String {
    let escaped: Cow<'_, str> = ldap_escape(username);
    template.replace("{username}", escaped.as_ref())
}

fn user_from_entry(entry: &SearchEntry, cfg: &LdapConfig) -> Result<LdapUser> {
    let subject = if cfg.field_mapping.subject.eq_ignore_ascii_case("dn") {
        entry.dn.clone()
    } else {
        first_attribute(entry, &cfg.field_mapping.subject).ok_or_else(|| {
            tracing::warn!(
                attribute = %cfg.field_mapping.subject,
                "LDAP user entry is missing the configured subject attribute"
            );
            Error::unauthorized("invalid LDAP credentials")
        })?
    };
    let Some(email) = first_attribute(entry, &cfg.field_mapping.email) else {
        tracing::warn!(
            attribute = %cfg.field_mapping.email,
            "LDAP user entry is missing the configured email attribute"
        );
        return Err(Error::unauthorized("invalid LDAP credentials"));
    };
    let email = email.to_ascii_lowercase();
    let display_name =
        first_attribute(entry, &cfg.field_mapping.display_name).unwrap_or_else(|| email.clone());
    let mut groups = attribute_values(entry, &cfg.field_mapping.groups)
        .into_iter()
        .flatten()
        .map(|group| group.trim())
        .filter(|group| !group.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    groups.sort();
    groups.dedup();
    Ok(LdapUser {
        subject,
        email,
        display_name,
        groups,
    })
}

fn attribute_values<'a>(entry: &'a SearchEntry, name: &str) -> Option<&'a Vec<String>> {
    entry
        .attrs
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, values)| values)
}

fn first_attribute(entry: &SearchEntry, name: &str) -> Option<String> {
    attribute_values(entry, name).and_then(|values| {
        values
            .iter()
            .map(|value| value.trim())
            .find(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

fn ldap_unavailable(operation: &'static str, error: impl std::fmt::Display) -> Error {
    tracing::warn!(operation, error = %error, "LDAP operation failed");
    Error::unavailable("LDAP authentication service unavailable")
}

#[cfg(test)]
mod tests {
    use super::{LdapConfig, LdapLoginFlow, render_user_filter};
    use crate::domain::iam::SsoFieldMapping;

    fn config() -> LdapConfig {
        LdapConfig {
            url: "ldaps://ldap.example.com:636".into(),
            start_tls: false,
            bind_dn: "cn=reader,dc=example,dc=com".into(),
            bind_password: "secret".into(),
            base_dn: "ou=people,dc=example,dc=com".into(),
            user_filter: "(&(objectClass=person)(mail={username}))".into(),
            field_mapping: SsoFieldMapping::ldap(),
        }
    }

    #[test]
    fn rejects_plaintext_ldap() {
        let mut cfg = config();
        cfg.url = "ldap://ldap.example.com:389".into();
        assert!(LdapLoginFlow::new(cfg).is_err());
    }

    #[test]
    fn accepts_starttls() {
        let mut cfg = config();
        cfg.url = "ldap://ldap.example.com:389".into();
        cfg.start_tls = true;
        assert!(LdapLoginFlow::new(cfg).is_ok());
    }

    #[test]
    fn escapes_filter_metacharacters() {
        let filter = render_user_filter("(uid={username})", "*)(uid=*)");
        assert_eq!(filter, r"(uid=\2a\29\28uid=\2a\29)");
    }

    #[test]
    fn requires_service_bind_pair() {
        let mut cfg = config();
        cfg.bind_password.clear();
        assert!(LdapLoginFlow::new(cfg).is_err());
    }
}
