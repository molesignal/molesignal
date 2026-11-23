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
use kernel::{Error, Result};

/// The language a key is read from when the requested language lacks it.
pub const FALLBACK_LANGUAGE_CODE: &str = "en-us";

/// The languages the product is written in: the interface, what the server
/// sends, and what its models are told to write. The web interface lists the
/// same codes in `web/src/i18n/locales.ts`.
pub const LANGUAGE_CODES: [&str; 9] = [
    "en-us", "zh-cn", "zh-tw", "ja-jp", "ko-kr", "de", "es", "fr", "pt-br",
];

/// The refusal for a language code the product is not written in.
pub const UNSUPPORTED_LANGUAGE: &str =
    "language must be one of en-us, zh-cn, zh-tw, ja-jp, ko-kr, de, es, fr, pt-br";

/// Whether `code` is one of [`LANGUAGE_CODES`], exactly as stored.
pub fn is_language_code(code: &str) -> bool {
    LANGUAGE_CODES.contains(&code)
}

/// The language code a language tag asks for: `zh-Hant-HK` is `zh-tw`, `pt-PT`
/// is `pt-br`, `ja` is `ja-jp`. An `Accept-Language` value is read in its
/// order. None when the product is written in none of the languages named.
pub fn match_language(tags: &str) -> Option<&'static str> {
    tags.split(',')
        .filter_map(|tag| tag.split(';').next())
        .find_map(match_tag)
}

fn match_tag(tag: &str) -> Option<&'static str> {
    let tag = tag.trim().to_ascii_lowercase().replace('_', "-");

    if let Some(code) = LANGUAGE_CODES.iter().find(|code| **code == tag) {
        return Some(code);
    }

    let mut parts = tag.split('-');
    let language = parts.next()?;

    match language {
        "zh" => {
            let traditional = parts.any(|part| matches!(part, "hant" | "tw" | "hk" | "mo"));
            Some(if traditional { "zh-tw" } else { "zh-cn" })
        }
        "en" => Some("en-us"),
        "ja" => Some("ja-jp"),
        "ko" => Some("ko-kr"),
        "de" => Some("de"),
        "es" => Some("es"),
        "fr" => Some("fr"),
        "pt" => Some("pt-br"),
        _ => None,
    }
}

/// The language code for a language tag, or [`FALLBACK_LANGUAGE_CODE`].
pub fn language_or_fallback(tags: Option<&str>) -> &'static str {
    tags.and_then(match_language)
        .unwrap_or(FALLBACK_LANGUAGE_CODE)
}

/// The English name of a language, as a model is told to write in it.
pub fn language_name(code: &str) -> Option<&'static str> {
    match code {
        "en-us" => Some("English"),
        "zh-cn" => Some("Simplified Chinese"),
        "zh-tw" => Some("Traditional Chinese as used in Taiwan"),
        "ja-jp" => Some("Japanese"),
        "ko-kr" => Some("Korean"),
        "de" => Some("German"),
        "es" => Some("Spanish"),
        "fr" => Some("French"),
        "pt-br" => Some("Brazilian Portuguese"),
        _ => None,
    }
}

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
    fn language_tags_resolve_to_the_languages_the_product_is_written_in() {
        assert_eq!(match_language("zh-CN"), Some("zh-cn"));
        assert_eq!(match_language("zh-Hant-HK"), Some("zh-tw"));
        assert_eq!(match_language("zh_TW"), Some("zh-tw"));
        assert_eq!(match_language("ja"), Some("ja-jp"));
        assert_eq!(match_language("pt-PT"), Some("pt-br"));
        assert_eq!(match_language("fr-CA"), Some("fr"));
        assert_eq!(match_language("nl-NL,de;q=0.8,en;q=0.5"), Some("de"));
        assert_eq!(match_language("nl"), None);
        assert_eq!(language_or_fallback(Some("ru")), "en-us");
        assert_eq!(language_or_fallback(None), "en-us");
    }

    #[test]
    fn every_language_has_a_name() {
        for code in LANGUAGE_CODES {
            assert!(is_language_code(code));
            assert!(language_name(code).is_some(), "{code}");
            assert_eq!(match_language(code), Some(code));
        }

        assert!(!is_language_code("fr-fr"));
    }

    #[test]
    fn a_missing_key_is_an_error_that_names_it() {
        let error = pack(&[]).render("email.subject", &[]).unwrap_err();
        assert!(error.to_string().contains("email.subject"));
    }
}
