// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The business context an organization reads its telemetry by: keys such as a tenant, a plan or
//! an experiment group, and for each the field that carries it in each signal. Applications put
//! the keys on their telemetry themselves, usually as W3C baggage that a baggage span or log
//! processor copies onto every Span and log record; the server rewrites nothing and only reads
//! the fields named here.
//!
//! Values of a key can be marked important, such as the customers that matter most: what touches
//! them is told as such wherever an incident's business impact is.

use async_trait::async_trait;
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde::{Deserialize, Serialize};

pub const MAX_BUSINESS_KEYS: usize = 8;
pub const MAX_IMPORTANT_VALUES: usize = 50;
const MAX_KEY_CHARS: usize = 64;
const MAX_LABEL_CHARS: usize = 80;
const MAX_FIELD_CHARS: usize = 128;
const MAX_VALUE_CHARS: usize = 200;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BusinessContext {
    #[serde(default)]
    pub keys: Vec<BusinessKey>,
}

/// One key and where each signal carries it. A signal that does not carry it names no field.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BusinessKey {
    /// What the key is called in links and by the agent: lower case, digits and `_`.
    pub key: String,
    /// What readers see it called.
    #[serde(default)]
    pub label: String,
    /// The Span attribute, as the traces stream names the field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<String>,
    /// The log field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log: Option<String>,
    /// The RUM field, a key of a JSON column included, such as `context.tenant`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rum: Option<String>,
    /// Values that matter most.
    #[serde(default)]
    pub important: Vec<String>,
}

impl BusinessContext {
    /// Trims what was typed: blank fields name nothing, a blank label is the key, and important
    /// values are listed once each.
    pub fn normalize(&mut self) {
        for key in &mut self.keys {
            key.key = key.key.trim().to_string();
            key.label = key.label.trim().to_string();

            if key.label.is_empty() {
                key.label.clone_from(&key.key);
            }

            for field in [&mut key.span, &mut key.log, &mut key.rum] {
                *field = field
                    .take()
                    .map(|name| name.trim().to_string())
                    .filter(|name| !name.is_empty());
            }

            let mut seen = std::collections::HashSet::new();

            key.important = std::mem::take(&mut key.important)
                .into_iter()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty() && seen.insert(value.clone()))
                .collect();
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.keys.len() > MAX_BUSINESS_KEYS {
            return Err(Error::invalid(format!(
                "at most {MAX_BUSINESS_KEYS} business keys can be declared"
            )));
        }

        let mut seen = std::collections::HashSet::new();

        for key in &self.keys {
            key.validate()?;

            if !seen.insert(key.key.as_str()) {
                return Err(Error::invalid(format!(
                    "business key `{}` is declared twice",
                    key.key
                )));
            }
        }

        Ok(())
    }

    /// The keys a signal carries, with the field it carries each in.
    pub fn carried_by(&self, signal: BusinessSignal) -> impl Iterator<Item = (&BusinessKey, &str)> {
        self.keys
            .iter()
            .filter_map(move |key| key.field(signal).map(|field| (key, field)))
    }
}

/// A signal a business key can be carried by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusinessSignal {
    Span,
    Log,
    Rum,
}

impl BusinessKey {
    pub fn field(&self, signal: BusinessSignal) -> Option<&str> {
        match signal {
            BusinessSignal::Span => self.span.as_deref(),
            BusinessSignal::Log => self.log.as_deref(),
            BusinessSignal::Rum => self.rum.as_deref(),
        }
    }

    pub fn is_important(&self, value: &str) -> bool {
        self.important.iter().any(|important| important == value)
    }

    fn validate(&self) -> Result<()> {
        let named = !self.key.is_empty()
            && self.key.len() <= MAX_KEY_CHARS
            && self
                .key
                .starts_with(|first: char| first.is_ascii_lowercase())
            && self.key.chars().all(|character| {
                character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
            });

        if !named {
            return Err(Error::invalid(format!(
                "business key `{}` must start with a lower-case letter and hold only lower-case \
                 letters, digits and `_`, at most {MAX_KEY_CHARS} of them",
                self.key
            )));
        }

        if self.label.chars().count() > MAX_LABEL_CHARS {
            return Err(Error::invalid(format!(
                "the label of business key `{}` is longer than {MAX_LABEL_CHARS} characters",
                self.key
            )));
        }

        let fields = [&self.span, &self.log, &self.rum];

        if fields.iter().all(|field| field.is_none()) {
            return Err(Error::invalid(format!(
                "business key `{}` names no field in any signal",
                self.key
            )));
        }

        for field in fields.into_iter().flatten() {
            if field.len() > MAX_FIELD_CHARS || field.chars().any(char::is_control) {
                return Err(Error::invalid(format!(
                    "business key `{}` names a field longer than {MAX_FIELD_CHARS} bytes or with \
                     control characters",
                    self.key
                )));
            }
        }

        if self.important.len() > MAX_IMPORTANT_VALUES {
            return Err(Error::invalid(format!(
                "business key `{}` marks more than {MAX_IMPORTANT_VALUES} values important",
                self.key
            )));
        }

        if self
            .important
            .iter()
            .any(|value| value.chars().count() > MAX_VALUE_CHARS)
        {
            return Err(Error::invalid(format!(
                "an important value of business key `{}` is longer than {MAX_VALUE_CHARS} \
                 characters",
                self.key
            )));
        }

        Ok(())
    }
}

#[async_trait]
pub trait BusinessContextRepository: Send + Sync {
    /// The organization's business context; none declared until it saves one.
    async fn business_context(&self, org_id: &Id) -> Result<BusinessContext>;

    async fn save_business_context(
        &self,
        org_id: &Id,
        context: &BusinessContext,
        updated_by: &Id,
        now: TimestampMicros,
    ) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(name: &str) -> BusinessKey {
        BusinessKey {
            key: name.into(),
            span: Some("tenant.id".into()),
            ..BusinessKey::default()
        }
    }

    #[test]
    fn what_was_typed_is_trimmed_and_a_blank_label_is_the_key() {
        let mut context = BusinessContext {
            keys: vec![BusinessKey {
                key: " tenant ".into(),
                label: "  ".into(),
                span: Some(" tenant.id ".into()),
                log: Some("   ".into()),
                rum: None,
                important: vec![" acme ".into(), "acme".into(), "".into(), "globex".into()],
            }],
        };

        context.normalize();

        let tenant = &context.keys[0];
        assert_eq!(tenant.key, "tenant");
        assert_eq!(tenant.label, "tenant");
        assert_eq!(tenant.span.as_deref(), Some("tenant.id"));
        assert_eq!(tenant.log, None);
        assert_eq!(tenant.important, ["acme", "globex"]);
        assert!(context.validate().is_ok());
    }

    #[test]
    fn a_key_is_named_plainly_once_and_carried_somewhere() {
        for name in ["Tenant", "1tenant", "ten-ant", ""] {
            let context = BusinessContext {
                keys: vec![key(name)],
            };
            assert!(context.validate().is_err(), "{name}");
        }

        let twice = BusinessContext {
            keys: vec![key("tenant"), key("tenant")],
        };
        assert!(twice.validate().is_err());

        let nowhere = BusinessContext {
            keys: vec![BusinessKey {
                key: "plan".into(),
                ..BusinessKey::default()
            }],
        };
        assert!(nowhere.validate().is_err());

        let too_many = BusinessContext {
            keys: (0..=MAX_BUSINESS_KEYS)
                .map(|index| key(&format!("k{index}")))
                .collect(),
        };
        assert!(too_many.validate().is_err());
    }

    #[test]
    fn the_keys_of_a_signal_come_with_their_field() {
        let context = BusinessContext {
            keys: vec![
                key("tenant"),
                BusinessKey {
                    key: "plan".into(),
                    rum: Some("context.plan".into()),
                    ..BusinessKey::default()
                },
            ],
        };

        let spans = context
            .carried_by(BusinessSignal::Span)
            .map(|(key, field)| (key.key.as_str(), field))
            .collect::<Vec<_>>();

        assert_eq!(spans, [("tenant", "tenant.id")]);
        assert_eq!(context.carried_by(BusinessSignal::Rum).count(), 1);
        assert_eq!(context.carried_by(BusinessSignal::Log).count(), 0);
    }
}
