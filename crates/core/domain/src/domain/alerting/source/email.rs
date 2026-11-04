// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 邮件来源怎么读一封邮件：接受谁发来的、什么主题算恢复、哪些不算告警、去重键怎么取。

use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result};

/// 一个来源最多列几个发件人。
pub const MAX_ALLOWED_SENDERS: usize = 64;

/// 正则最多几个字符。
const MAX_PATTERN_CHARS: usize = 512;

/// 默认算恢复的主题：常见监控系统的恢复邮件。`OK` 只认开头，免得「Check OK failed」算恢复。
pub const DEFAULT_RESOLVE_PATTERN: &str =
    r"(?i)(^\W*ok\b|\b(resolved|recovered|recovery|cleared)\b|恢复|已解决)";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailRules {
    /// 接受谁发来的邮件：完整地址，或 `@example.com` 整个域，`*` 是谁都接受。空着时不看发件人；配了
    /// 邮箱的来源不能空着，见 [`MailboxRepository`](super::MailboxRepository) 的保存。
    #[serde(default)]
    pub allowed_senders: Vec<String>,
    /// 主题符合时是恢复；空着用 [`DEFAULT_RESOLVE_PATTERN`]。
    #[serde(default)]
    pub resolve_pattern: String,
    /// 主题符合时不算告警，直接跳过；空着不跳过。
    #[serde(default)]
    pub ignore_pattern: String,
    /// 从主题里取去重键的正则：第一个分组（没有分组时是整个匹配）。空着或没匹配到时用整理过的主题。
    #[serde(default)]
    pub dedup_pattern: String,
}

impl EmailRules {
    pub fn validate(&self) -> Result<()> {
        if self.allowed_senders.len() > MAX_ALLOWED_SENDERS {
            return Err(Error::invalid(format!(
                "a source lists at most {MAX_ALLOWED_SENDERS} senders"
            )));
        }

        for sender in &self.allowed_senders {
            let sender = sender.trim();

            let well_formed = sender == "*"
                || sender
                    .split_once('@')
                    .is_some_and(|(_, domain)| domain.contains('.') && !domain.contains('@'));

            if !well_formed {
                return Err(Error::invalid(format!(
                    "`{sender}` is not a sender: write an address, `@example.com` or `*`"
                )));
            }
        }

        for (name, pattern) in [
            ("resolve_pattern", &self.resolve_pattern),
            ("ignore_pattern", &self.ignore_pattern),
            ("dedup_pattern", &self.dedup_pattern),
        ] {
            if pattern.chars().count() > MAX_PATTERN_CHARS {
                return Err(Error::invalid(format!(
                    "{name} has at most {MAX_PATTERN_CHARS} characters"
                )));
            }

            if !pattern.trim().is_empty() && regex::Regex::new(pattern).is_err() {
                return Err(Error::invalid(format!(
                    "{name} is not a regular expression that parses"
                )));
            }
        }

        Ok(())
    }

    /// 这个发件人收不收。不区分大小写；`allowed_senders` 空着时都收。
    pub fn accepts(&self, sender: &str) -> bool {
        if self.allowed_senders.is_empty() {
            return true;
        }

        let sender = sender.trim().to_ascii_lowercase();

        self.allowed_senders.iter().any(|allowed| {
            let allowed = allowed.trim().to_ascii_lowercase();

            match allowed.as_str() {
                "*" => true,
                domain if domain.starts_with('@') => sender.ends_with(domain),
                address => sender == address,
            }
        })
    }

    /// 这封邮件一定要说明白接受谁：配了邮箱的来源，地址可能被任何人知道。
    pub fn names_senders(&self) -> bool {
        self.allowed_senders
            .iter()
            .any(|sender| !sender.trim().is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(senders: &[&str]) -> EmailRules {
        EmailRules {
            allowed_senders: senders.iter().map(ToString::to_string).collect(),
            ..EmailRules::default()
        }
    }

    #[test]
    fn senders_match_by_address_domain_or_everyone() {
        let by_address = rules(&["Alerts@Acme.com"]);
        assert!(by_address.accepts("alerts@acme.com"));
        assert!(!by_address.accepts("other@acme.com"));

        let by_domain = rules(&["@monitoring.acme.com"]);
        assert!(by_domain.accepts("zabbix@monitoring.acme.com"));
        assert!(!by_domain.accepts("zabbix@evil-monitoring.acme.com.cn"));
        assert!(!by_domain.accepts("zabbix@notmonitoring.acme.com"));

        assert!(rules(&["*"]).accepts("anyone@anywhere.org"));
        assert!(rules(&[]).accepts("anyone@anywhere.org"));
    }

    #[test]
    fn the_rules_must_parse() {
        assert!(rules(&["a@b.com", "@c.org", "*"]).validate().is_ok());
        assert!(rules(&["not an address"]).validate().is_err());
        assert!(rules(&["@localhost"]).validate().is_err());

        let bad = EmailRules {
            resolve_pattern: "(".into(),
            ..EmailRules::default()
        };
        assert!(bad.validate().is_err());
        assert!(regex::Regex::new(DEFAULT_RESOLVE_PATTERN).is_ok());
    }
}
