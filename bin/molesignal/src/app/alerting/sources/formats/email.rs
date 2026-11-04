// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 邮件告警：一封原始邮件（RFC 822）读成一条告警。IMAP 邮箱读到的和 `message/rfc822` 发来的都走这里。
//!
//! - 发件人不在来源的名单里：拒绝，留一条记录。
//! - 自动回复和退信不算告警（退信是发给发件人的，读别人的邮箱时会混进来）。
//! - 主题符合恢复的写法算恢复（[`DEFAULT_RESOLVE_PATTERN`]，来源上可以改），其余算触发。
//! - 去重键：发件人的域，加上主题里来源规则取出的部分，或整理过的主题：去掉 `Re:`、`Fwd:`、状态词、
//!   严重度、时间和百分比，大小写不算。同一个告警的「问题」和「恢复」两封邮件落到同一个键上。
//! - 严重度看主题里的词，没有用来源的默认严重度。

use std::{collections::BTreeMap, sync::LazyLock};

use mail_parser::{Message, MessageParser, MimeHeaders};
use regex::Regex;

use super::Parsed;
use crate::{
    domain::alerting::{
        incident::Severity,
        source::{
            DEFAULT_RESOLVE_PATTERN, EmailRules, ExternalAction, ExternalAlert, MAX_DEDUP_KEY_CHARS,
        },
    },
    shared::{Error, Result, time::TimestampMicros},
};

/// 开头的回复、转发前缀，可以叠好几层。
static REPLY_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*(re|fw|fwd|aw|wg|回复|答复|转发)\s*[:：]\s*").expect("reply prefix")
});

/// 状态词和严重度：同一个告警在问题、恢复、升级的邮件里不一样的部分。
static STATUS_WORDS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(problem|alert|alarm|firing|triggered|resolved|recovered|recovery|cleared|ok|critical|crit|warning|warn|error|err|info|notice|fatal|emergency|disaster|p[1-5]|sev[-_ ]?[1-5])\b|告警|报警|恢复|已解决|严重|警告|故障",
    )
    .expect("status words")
});

/// 时间和百分比：每封邮件都不一样，但不是另一个告警。
static CHANGING_VALUES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\d{4}[-/.]\d{1,2}[-/.]\d{1,2}(?:[ T]\d{1,2}:\d{2}(?::\d{2})?(?:\.\d+)?Z?)?|\b\d{1,2}:\d{2}(?::\d{2})?\b|\d+(?:\.\d+)?\s*%")
        .expect("changing values")
});

/// 主题里认作严重度的词。不认 `high`、`low`：「High CPU」「Low disk」说的不是严重度。
static SEVERITY_WORDS: LazyLock<[(Severity, Regex); 4]> = LazyLock::new(|| {
    let word = |words: &str| Regex::new(&format!(r"(?i)(\b({words})\b)")).expect("severity words");

    [
        (
            Severity::Critical,
            word("critical|crit|fatal|emergency|disaster|p1|sev[-_ ]?1"),
        ),
        (Severity::Error, word("error|p2|sev[-_ ]?2")),
        (Severity::Warning, word("warning|warn|p3|sev[-_ ]?3")),
        (Severity::Info, word("info|notice|p4|p5|sev[-_ ]?[45]")),
    ]
});

pub(super) fn parse(rules: &EmailRules, body: &[u8]) -> Result<Parsed> {
    let message = MessageParser::default()
        .parse(body)
        .filter(|message| !message.raw_message().is_empty())
        .ok_or_else(|| Error::invalid("the request body is not an email message"))?;

    let from = message
        .from()
        .and_then(|from| from.first())
        .and_then(|from| from.address())
        .unwrap_or_default()
        .trim()
        .to_string();

    if !rules.accepts(&from) {
        return Err(Error::invalid(format!(
            "`{from}` is not among the senders of this alert source"
        )));
    }

    if is_automatic_reply(&message) {
        return Ok(Parsed::nothing("an automatic reply or a bounce"));
    }

    let raw_subject = message.subject().unwrap_or_default();
    let summary = without_reply_prefixes(raw_subject);

    if let Some(ignore) = compile(&rules.ignore_pattern)?
        && ignore.is_match(&summary)
    {
        return Ok(Parsed::nothing("the subject matches the ignore pattern"));
    }

    let resolve_pattern = if rules.resolve_pattern.trim().is_empty() {
        DEFAULT_RESOLVE_PATTERN
    } else {
        &rules.resolve_pattern
    };

    let resolves = compile(resolve_pattern)?.is_some_and(|resolve| resolve.is_match(&summary));

    let action = if resolves {
        ExternalAction::Resolve
    } else {
        ExternalAction::Trigger
    };

    let domain = from
        .rsplit_once('@')
        .map(|(_, domain)| domain.to_ascii_lowercase())
        .unwrap_or_default();

    let key = dedup_part(&rules.dedup_pattern, &summary)?;
    let dedup_key: String = format!("{domain}|{key}")
        .chars()
        .take(MAX_DEDUP_KEY_CHARS)
        .collect();

    let mut labels = BTreeMap::new();
    labels.insert("email.from".to_string(), from.clone());

    let mut annotations = BTreeMap::new();

    if let Some(text) = message.body_text(0).or_else(|| message.body_html(0)) {
        annotations.insert("message".to_string(), text.trim().to_string());
    }

    annotations.insert("from".to_string(), from.clone());

    if let Some(id) = message.message_id() {
        annotations.insert("message_id".to_string(), id.to_string());
    }

    let summary = if summary.is_empty() {
        format!("Email from {from}")
    } else {
        summary
    };

    let occurred_at = message
        .date()
        .map(|date| TimestampMicros::from_secs(date.to_timestamp()))
        .filter(|at| at.0 > 0);

    Ok(Parsed::alerts(vec![ExternalAlert {
        action,
        dedup_key,
        severity: severity_in(&summary),
        summary,
        labels,
        annotations,
        occurred_at,
        no_data: false,
        members: Vec::new(),
        priority: None,
        urgency: None,
    }]))
}

fn compile(pattern: &str) -> Result<Option<Regex>> {
    let pattern = pattern.trim();

    if pattern.is_empty() {
        return Ok(None);
    }

    Regex::new(pattern)
        .map(Some)
        .map_err(|_| Error::invalid("a pattern of this alert source does not parse"))
}

fn without_reply_prefixes(subject: &str) -> String {
    let mut rest = subject.trim();

    while let Some(found) = REPLY_PREFIX.find(rest) {
        rest = &rest[found.end()..];
    }

    rest.trim().to_string()
}

/// 去重键里主题的部分：规则取得出就用规则取的，否则用整理过的主题。
fn dedup_part(pattern: &str, summary: &str) -> Result<String> {
    if let Some(regex) = compile(pattern)?
        && let Some(captures) = regex.captures(summary)
        && let Some(found) = captures.get(1).or_else(|| captures.get(0))
    {
        return Ok(found.as_str().trim().to_lowercase());
    }

    Ok(normalized(summary))
}

/// 整理过的主题：去掉状态词、严重度、时间和百分比，标点和空白并成一个空格，小写。
fn normalized(summary: &str) -> String {
    let without_values = CHANGING_VALUES.replace_all(summary, " ");
    let without_words = STATUS_WORDS.replace_all(&without_values, " ");

    without_words
        .split(|c: char| !c.is_alphanumeric() && !matches!(c, '-' | '_' | '.'))
        .map(|part| part.trim_matches(['-', '_', '.']))
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn severity_in(subject: &str) -> Option<Severity> {
    SEVERITY_WORDS
        .iter()
        .find(|(_, words)| words.is_match(subject))
        .map(|(severity, _)| *severity)
}

/// 自动回复和退信。监控系统发的邮件常带 `Auto-Submitted: auto-generated`，那是告警，不跳过。
fn is_automatic_reply(message: &Message<'_>) -> bool {
    let header = |name: &str| {
        message
            .header_raw(name)
            .map(|value| value.trim().to_ascii_lowercase())
            .unwrap_or_default()
    };

    let report = message.content_type().is_some_and(|content_type| {
        content_type.ctype().eq_ignore_ascii_case("multipart")
            && content_type
                .subtype()
                .is_some_and(|subtype| subtype.eq_ignore_ascii_case("report"))
    });

    report
        || header("Auto-Submitted").starts_with("auto-replied")
        || header("X-Autoreply") == "yes"
        || header("Precedence") == "auto_reply"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mail(from: &str, subject: &str, body: &str) -> Vec<u8> {
        format!(
            "From: Monitor <{from}>\r\nTo: alerts@acme.com\r\nSubject: {subject}\r\n\
             Date: Mon, 27 Oct 2025 08:00:00 +0800\r\nMessage-ID: <m1@acme.com>\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\r\n{body}\r\n"
        )
        .into_bytes()
    }

    fn rules() -> EmailRules {
        EmailRules {
            allowed_senders: vec!["@monitor.acme.com".into()],
            ..EmailRules::default()
        }
    }

    fn one(subject: &str) -> ExternalAlert {
        let parsed = parse(
            &rules(),
            &mail("nagios@monitor.acme.com", subject, "Disk is full"),
        )
        .unwrap();
        parsed.alerts.into_iter().next().unwrap()
    }

    #[test]
    fn a_mail_opens_an_alert_with_its_subject_body_and_time() {
        let alert = one("PROBLEM: disk full on web-1");

        assert_eq!(alert.action, ExternalAction::Trigger);
        assert_eq!(alert.summary, "PROBLEM: disk full on web-1");
        assert_eq!(alert.dedup_key, "monitor.acme.com|disk full on web-1");
        assert_eq!(alert.severity, None);
        assert_eq!(alert.labels["email.from"], "nagios@monitor.acme.com");
        assert_eq!(alert.annotations["message"], "Disk is full");
        assert_eq!(alert.annotations["message_id"], "m1@acme.com");
        assert_eq!(
            alert.occurred_at,
            Some(TimestampMicros::from_secs(1_761_523_200))
        );
    }

    #[test]
    fn the_recovery_mail_lands_on_the_same_key() {
        let problem = one("CRITICAL: disk full on web-1 at 2025-10-27 08:00:00 (91%)");
        let recovery = one("Re: RECOVERY: disk full on web-1 at 2025-10-27 09:15:00 (40%)");

        assert_eq!(problem.action, ExternalAction::Trigger);
        assert_eq!(problem.severity, Some(Severity::Critical));
        assert_eq!(recovery.action, ExternalAction::Resolve);
        assert_eq!(problem.dedup_key, recovery.dedup_key);
        assert_eq!(
            recovery.summary,
            "RECOVERY: disk full on web-1 at 2025-10-27 09:15:00 (40%)"
        );

        let chinese_problem = one("【告警】订单服务 5xx 过高");
        let chinese_recovery = one("【恢复】订单服务 5xx 过高");
        assert_eq!(chinese_recovery.action, ExternalAction::Resolve);
        assert_eq!(chinese_problem.dedup_key, chinese_recovery.dedup_key);
    }

    #[test]
    fn different_hosts_stay_different_alerts() {
        assert_ne!(
            one("PROBLEM: disk full on web-1").dedup_key,
            one("PROBLEM: disk full on web-2").dedup_key
        );
    }

    #[test]
    fn ok_counts_only_at_the_start() {
        assert_eq!(one("OK: disk on web-1").action, ExternalAction::Resolve);
        assert_eq!(one("Check OK failed").action, ExternalAction::Trigger);
    }

    #[test]
    fn severity_words_do_not_mistake_high_cpu_for_a_severity() {
        assert_eq!(one("WARNING high CPU").severity, Some(Severity::Warning));
        assert_eq!(one("High CPU on web-1").severity, None);
        assert_eq!(one("Low disk space").severity, None);
        assert_eq!(one("[P2] Checkout errors").severity, Some(Severity::Error));
    }

    #[test]
    fn a_rule_can_take_the_key_and_the_recovery_from_the_subject() {
        let rules = EmailRules {
            allowed_senders: vec!["*".into()],
            resolve_pattern: r"\bUP\b".into(),
            dedup_pattern: r"host=(\S+)".into(),
            ignore_pattern: r"^\[test\]".into(),
        };

        let parsed = parse(&rules, &mail("x@y.com", "DOWN host=web-1", "")).unwrap();
        assert_eq!(parsed.alerts[0].dedup_key, "y.com|web-1");
        assert_eq!(parsed.alerts[0].action, ExternalAction::Trigger);

        let up = parse(&rules, &mail("x@y.com", "UP host=web-1 (RECOVERY)", "")).unwrap();
        assert_eq!(up.alerts[0].dedup_key, "y.com|web-1");
        assert_eq!(up.alerts[0].action, ExternalAction::Resolve);

        let ignored = parse(&rules, &mail("x@y.com", "[test] anything", "")).unwrap();
        assert!(ignored.alerts.is_empty());
        assert!(ignored.message.is_some());
    }

    #[test]
    fn a_sender_not_on_the_list_is_refused() {
        let error = parse(&rules(), &mail("stranger@elsewhere.org", "PROBLEM", ""))
            .unwrap_err()
            .to_string();

        assert!(error.contains("stranger@elsewhere.org"), "{error}");
    }

    #[test]
    fn automatic_replies_and_bounces_are_no_alerts() {
        let auto_reply = "From: a@monitor.acme.com\r\nSubject: Out of office\r\n\
             Auto-Submitted: auto-replied\r\n\r\nAway\r\n";
        let parsed = parse(&rules(), auto_reply.as_bytes()).unwrap();
        assert!(parsed.alerts.is_empty());

        let bounce = "From: a@monitor.acme.com\r\nSubject: Undelivered\r\n\
             Content-Type: multipart/report; report-type=delivery-status; boundary=b\r\n\r\n\
             --b\r\nContent-Type: text/plain\r\n\r\nfailed\r\n--b--\r\n";
        assert!(
            parse(&rules(), bounce.as_bytes())
                .unwrap()
                .alerts
                .is_empty()
        );

        // 监控系统自动发的是告警。
        let generated = "From: a@monitor.acme.com\r\nSubject: PROBLEM x\r\n\
             Auto-Submitted: auto-generated\r\n\r\nbody\r\n";
        assert_eq!(
            parse(&rules(), generated.as_bytes()).unwrap().alerts.len(),
            1
        );
    }

    #[test]
    fn encoded_subjects_and_gbk_bodies_are_read() {
        let raw = b"From: a@monitor.acme.com\r\n\
            Subject: =?GB2312?B?1tDOxLjmvq8=?=\r\n\
            Content-Type: text/plain; charset=gbk\r\n\
            Content-Transfer-Encoding: base64\r\n\r\n1tDOxA==\r\n";

        let parsed = parse(&rules(), raw).unwrap();

        assert_eq!(parsed.alerts[0].summary, "中文告警");
        assert_eq!(parsed.alerts[0].annotations["message"], "中文");
    }

    #[test]
    fn an_html_only_mail_is_read_as_text() {
        let raw = "From: a@monitor.acme.com\r\nSubject: PROBLEM x\r\n\
            Content-Type: text/html; charset=utf-8\r\n\r\n<p>Disk <b>full</b></p>\r\n";

        let parsed = parse(&rules(), raw.as_bytes()).unwrap();
        let message = &parsed.alerts[0].annotations["message"];

        assert!(message.contains("Disk"), "{message}");
        assert!(message.contains("full"), "{message}");
    }

    #[test]
    fn something_that_is_no_mail_is_refused() {
        assert!(parse(&rules(), b"").is_err());
    }
}
