// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Intelligence PII 脱敏：email / phone / credit card。

use once_cell::sync::Lazy;
use regex::Regex;

static RE_EMAIL: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}").unwrap());
static RE_PHONE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\+?\d[\d\s().-]{8,}\d").unwrap());
static RE_CC: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b\d{13,19}\b").unwrap());

pub fn redact_pii(input: &str) -> String {
    let s = RE_EMAIL.replace_all(input, "<email>").into_owned();
    let s = RE_CC.replace_all(&s, "<cc>").into_owned();
    RE_PHONE.replace_all(&s, "<phone>").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_email_phone_cc() {
        let s = "contact me at foo@bar.com or +1 (415) 555-1212; card 4111111111111111";
        let out = redact_pii(s);
        assert!(out.contains("<email>"));
        assert!(out.contains("<phone>"));
        assert!(out.contains("<cc>"));
    }

    #[test]
    fn empty_input() {
        assert_eq!(redact_pii(""), "");
    }
}
