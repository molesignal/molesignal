// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Rules that group errors otherwise than by their type, frame and endpoint.
//! The errors a rule matches in a service become one group named by the
//! rule, or form no group at all; the requests that failed with them still
//! count as failed. Rules are tried in order and the first that matches
//! holds. They apply to errors seen after they are saved.

use serde::{Deserialize, Serialize};

use super::{ErrorIdentity, ServiceIdentity};
use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

/// The most rules an organization keeps.
pub const ERROR_RULES_MAX: usize = 100;
pub const ERROR_RULE_NAME_MAX_LENGTH: usize = 128;
pub const ERROR_RULE_PATTERN_MAX_LENGTH: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorRuleAction {
    /// The errors it matches in a service are one group, named by the rule.
    Merge,
    /// The errors it matches form no group.
    Ignore,
}

impl ErrorRuleAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Ignore => "ignore",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "merge" => Some(Self::Merge),
            "ignore" => Some(Self::Ignore),
            _ => None,
        }
    }
}

/// What an error must look like to be grouped by the rule. Each pattern
/// matches the whole text, ignoring case, with `*` standing for any text; an
/// empty one matches anything. The service is matched exactly; an empty
/// name matches every service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorRule {
    pub id: Id,
    pub name: String,
    #[serde(default)]
    pub service_namespace: String,
    #[serde(default)]
    pub service_name: String,
    #[serde(default)]
    pub error_type: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub frame: String,
    #[serde(default)]
    pub endpoint: String,
    pub action: ErrorRuleAction,
    pub enabled: bool,
    /// Where it is tried among the organization's rules, the lowest first.
    pub position: u32,
    pub updated_by: Id,
    pub updated_at: TimestampMicros,
}

impl ErrorRule {
    pub fn validate(&self) -> Result<()> {
        let name = self.name.trim();

        if name.is_empty() || name.chars().count() > ERROR_RULE_NAME_MAX_LENGTH {
            return Err(Error::invalid(format!(
                "an error rule needs a name of at most {ERROR_RULE_NAME_MAX_LENGTH} characters"
            )));
        }

        if !self.service_namespace.is_empty() && self.service_name.is_empty() {
            return Err(Error::invalid(
                "an error rule names a service along with its namespace",
            ));
        }

        if self.service_namespace.chars().count() > 128 || self.service_name.chars().count() > 255 {
            return Err(Error::invalid("the service namespace or name is too long"));
        }

        let patterns = self.patterns();

        if patterns.iter().all(|pattern| pattern.is_empty()) {
            return Err(Error::invalid(
                "an error rule needs a pattern for the error type, message, frame or endpoint",
            ));
        }

        if patterns
            .iter()
            .any(|pattern| pattern.chars().count() > ERROR_RULE_PATTERN_MAX_LENGTH)
        {
            return Err(Error::invalid(format!(
                "an error rule pattern has at most {ERROR_RULE_PATTERN_MAX_LENGTH} characters"
            )));
        }

        Ok(())
    }

    /// Whether the rule takes this error of `service`, whose exception said
    /// `message`.
    pub fn matches(
        &self,
        service: &ServiceIdentity,
        error: &ErrorIdentity,
        message: Option<&str>,
    ) -> bool {
        let service_matches = self.service_name.is_empty()
            || (self.service_name == service.name
                && (self.service_namespace.is_empty()
                    || self.service_namespace == service.namespace));

        service_matches
            && self.enabled
            && field_matches(&self.error_type, Some(&error.error_type))
            && field_matches(&self.message, message)
            && field_matches(&self.frame, error.application_frame.as_deref())
            && field_matches(&self.endpoint, error.transaction_name.as_deref())
    }

    fn patterns(&self) -> [&str; 4] {
        [
            self.error_type.as_str(),
            self.message.as_str(),
            self.frame.as_str(),
            self.endpoint.as_str(),
        ]
    }
}

fn field_matches(pattern: &str, value: Option<&str>) -> bool {
    pattern.is_empty() || value.is_some_and(|value| pattern_matches(pattern, value))
}

/// Whether `pattern` matches the whole of `text`, ignoring case, `*` standing
/// for any text, none included.
pub fn pattern_matches(pattern: &str, text: &str) -> bool {
    let pattern = pattern.to_lowercase().chars().collect::<Vec<_>>();
    let text = text.to_lowercase().chars().collect::<Vec<_>>();
    let (mut at, mut from) = (0, 0);
    // Where the last `*` was, and where in the text it began to stand.
    let mut star = None::<(usize, usize)>;

    while at < text.len() {
        if from < pattern.len() && pattern[from] == '*' {
            star = Some((from, at));
            from += 1;
        } else if from < pattern.len() && pattern[from] == text[at] {
            from += 1;
            at += 1;
        } else if let Some((star_at, stood)) = star {
            from = star_at + 1;
            at = stood + 1;
            star = Some((star_at, stood + 1));
        } else {
            return false;
        }
    }

    pattern[from..].iter().all(|char| *char == '*')
}

/// The rules in the order they are tried.
pub fn ordered_error_rules(mut rules: Vec<ErrorRule>) -> Vec<ErrorRule> {
    rules.sort_by(|left, right| {
        left.position
            .cmp(&right.position)
            .then_with(|| left.id.as_str().cmp(right.id.as_str()))
    });

    rules
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule() -> ErrorRule {
        ErrorRule {
            id: Id::from_string("rule-1"),
            name: "Timeouts".into(),
            service_namespace: String::new(),
            service_name: String::new(),
            error_type: "*Timeout*".into(),
            message: String::new(),
            frame: String::new(),
            endpoint: String::new(),
            action: ErrorRuleAction::Merge,
            enabled: true,
            position: 0,
            updated_by: Id::from_string("user"),
            updated_at: TimestampMicros(1),
        }
    }

    fn error(error_type: &str) -> ErrorIdentity {
        ErrorIdentity {
            fingerprint: "fp".into(),
            error_type: error_type.into(),
            application_frame: Some("app.Client.call".into()),
            transaction_name: Some("GET /orders".into()),
            overflow: false,
        }
    }

    #[test]
    fn a_pattern_matches_the_whole_text_with_stars_for_any_text() {
        assert!(pattern_matches("*timeout*", "ReadTimeoutError"));
        assert!(pattern_matches("GET /orders/*", "get /orders/{id}"));
        assert!(pattern_matches("a*b*c", "aXXbYYc"));
        assert!(pattern_matches("*", ""));
        assert!(pattern_matches("abc*", "abc"));
        assert!(!pattern_matches("abc", "abcd"));
        assert!(!pattern_matches("a*b", "acd"));
        assert!(!pattern_matches("", "a"));
    }

    #[test]
    fn a_rule_takes_the_errors_its_patterns_and_service_match() {
        let checkout = ServiceIdentity::new(Some("shop"), Some("checkout"), Some("prod"), None);
        let cart = ServiceIdentity::new(Some("shop"), Some("cart"), Some("prod"), None);
        let mut rule = rule();

        assert!(rule.matches(&checkout, &error("SocketTimeoutException"), None));
        assert!(!rule.matches(&checkout, &error("NullPointerException"), None));

        rule.service_name = "cart".into();

        assert!(!rule.matches(&checkout, &error("SocketTimeoutException"), None));
        assert!(rule.matches(&cart, &error("SocketTimeoutException"), None));

        rule.message = "*upstream*".into();

        assert!(!rule.matches(&cart, &error("SocketTimeoutException"), None));
        assert!(rule.matches(
            &cart,
            &error("SocketTimeoutException"),
            Some("Upstream took too long"),
        ));

        rule.enabled = false;

        assert!(!rule.matches(
            &cart,
            &error("SocketTimeoutException"),
            Some("Upstream took too long"),
        ));
    }

    #[test]
    fn a_rule_needs_a_name_and_a_pattern() {
        assert!(rule().validate().is_ok());

        let nameless = ErrorRule {
            name: " ".into(),
            ..rule()
        };
        let patternless = ErrorRule {
            error_type: String::new(),
            ..rule()
        };
        let namespace_alone = ErrorRule {
            service_namespace: "shop".into(),
            ..rule()
        };

        assert!(nameless.validate().is_err());
        assert!(patternless.validate().is_err());
        assert!(namespace_alone.validate().is_err());
    }
}
