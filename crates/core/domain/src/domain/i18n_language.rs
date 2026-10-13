// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Text the backend writes itself, kept as data instead of in the source.
//!
//! An entry is addressed by a dotted key and a lower-case language code such
//! as `en-us` or `zh-cn`. `{name}` in its value marks what the caller fills in.
//! The web interface does not read these entries: it keeps its own translation
//! files.

use std::collections::HashMap;

use async_trait::async_trait;

use crate::shared::{Error, Result};

/// The language a key is read from when the requested language lacks it.
pub const FALLBACK_LANGUAGE_CODE: &str = "en-us";

/// The entries of one language under one key prefix.
#[derive(Debug, Clone, Default)]
pub struct LanguagePack {
    values: HashMap<String, String>,
}

impl LanguagePack {
    pub fn new(values: HashMap<String, String>) -> Self {
        Self { values }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    /// Writes the value under `key`, replacing every `{name}` that `fills` names.
    ///
    /// A fill is inserted as it is and is not searched for placeholders, so
    /// text written by a user cannot pull in another fill. A placeholder that
    /// `fills` does not name stays in the output.
    pub fn render(&self, key: &str, fills: &[(&str, &str)]) -> Result<String> {
        let value = self
            .get(key)
            .ok_or_else(|| Error::internal(format!("i18n language key {key} is missing")))?;

        Ok(fill_placeholders(value, fills))
    }
}

fn fill_placeholders(value: &str, fills: &[(&str, &str)]) -> String {
    let mut written = String::with_capacity(value.len());
    let mut rest = value;

    while let Some(open) = rest.find('{') {
        written.push_str(&rest[..open]);
        let after_brace = &rest[open + 1..];

        let named = after_brace.find('}').and_then(|close| {
            let name = &after_brace[..close];

            fills
                .iter()
                .find(|(candidate, _)| *candidate == name)
                .map(|(_, fill)| (*fill, &after_brace[close + 1..]))
        });

        match named {
            Some((fill, after_placeholder)) => {
                written.push_str(fill);
                rest = after_placeholder;
            }
            None => {
                written.push('{');
                rest = after_brace;
            }
        }
    }

    written.push_str(rest);
    written
}

/// Where the entries are stored.
#[async_trait]
pub trait I18nLanguageRepository: Send + Sync {
    /// The entries whose key starts with `key_prefix`, in `language_code`. A
    /// key that the language lacks is read from [`FALLBACK_LANGUAGE_CODE`].
    async fn load_pack(&self, key_prefix: &str, language_code: &str) -> Result<LanguagePack>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(entries: &[(&str, &str)]) -> LanguagePack {
        LanguagePack::new(
            entries
                .iter()
                .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
                .collect(),
        )
    }

    #[test]
    fn render_fills_every_named_placeholder() {
        let pack = pack(&[("greeting", "{name}, {name}: {count} updates")]);

        assert_eq!(
            pack.render("greeting", &[("name", "Acme"), ("count", "2")])
                .unwrap(),
            "Acme, Acme: 2 updates"
        );
    }

    #[test]
    fn inserted_fills_are_not_searched_for_placeholders() {
        let pack = pack(&[("update", "{title}\n{body}\n{page_url}")]);

        let written = pack
            .render(
                "update",
                &[
                    ("title", "{page_url}"),
                    ("body", "see {title}"),
                    ("page_url", "https://status.example"),
                ],
            )
            .unwrap();

        assert_eq!(written, "{page_url}\nsee {title}\nhttps://status.example");
    }

    #[test]
    fn unknown_and_unclosed_placeholders_stay_in_the_text() {
        let pack = pack(&[("odd", "{missing} {page} { {page")]);

        assert_eq!(
            pack.render("odd", &[("page", "Acme")]).unwrap(),
            "{missing} Acme { {page"
        );
    }

    #[test]
    fn a_missing_key_is_an_error_that_names_it() {
        let error = pack(&[]).render("email.subject", &[]).unwrap_err();
        assert!(error.to_string().contains("email.subject"));
    }
}
