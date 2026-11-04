// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 电话与短信连接器共用的部分：号码的写法（E.164），以及一条通知在电话里念什么、短信里发什么。

use crate::{
    domain::notify::connector::{NotifyMessage, NotifyTarget, NotifyTargetType},
    shared::{Error, Result},
};

/// 同一个号码一分钟只拨一次，其间的通知合并成下一通（阿里云对同一号码本来就是每分钟一通）。
pub(crate) const CALL_MERGE_WINDOW_SECS: u32 = 60;

/// 一通电话念的字数上限：再长，接电话的人也听不完。
pub(crate) const MAX_SPOKEN_CHARS: usize = 400;

/// 一条短信的字数上限：两段以内。
pub(crate) const MAX_TEXT_CHARS: usize = 320;

/// 号码写成 E.164：`+`、国家码和号码，共 8 到 15 位数字。空格、短横线、括号与点都去掉，`00`
/// 开头的国际写法换成 `+`。
pub(crate) fn e164(value: &str) -> Result<String> {
    let compact = value
        .trim()
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '(' | ')' | '.'))
        .collect::<String>();

    let compact = match compact.strip_prefix("00") {
        Some(rest) => format!("+{rest}"),
        None => compact,
    };

    let digits = compact.strip_prefix('+').unwrap_or_default();

    let well_formed = (8..=15).contains(&digits.len())
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && !digits.starts_with('0');

    if !well_formed {
        return Err(Error::invalid(
            "a phone number starts with + and the country code, like +8613800138000",
        ));
    }

    Ok(compact)
}

/// 阿里云还认不带国家码的 11 位内地手机号，按 +86 读。
pub(crate) fn e164_or_mainland_mobile(value: &str) -> Result<String> {
    let trimmed = value.trim();

    if is_mainland_mobile(trimmed) {
        return Ok(format!("+86{trimmed}"));
    }

    e164(trimmed)
}

/// 内地号码在国内拨的写法：手机号 11 位，固话前面加 0（如 010、0571 开头）。不是 +86 的号码打不了。
pub(crate) fn mainland_dial(e164: &str) -> Result<String> {
    let Some(national) = e164.strip_prefix("+86") else {
        return Err(Error::invalid(
            "calls go to numbers in mainland China (+86) only",
        ));
    };

    if is_mainland_mobile(national) {
        Ok(national.to_string())
    } else {
        Ok(format!("0{national}"))
    }
}

/// 内地手机号：11 位，1 开头，第二位 3 到 9。
fn is_mainland_mobile(value: &str) -> bool {
    let bytes = value.as_bytes();

    bytes.len() == 11
        && bytes[0] == b'1'
        && (b'3'..=b'9').contains(&bytes[1])
        && bytes.iter().all(u8::is_ascii_digit)
}

/// 电话与短信只发给一个号码：个人的号码（`direct_user`），或团队、组织兜底里的固定号码
/// （`fixed_address`）。`read` 是这个连接器认的写法。
pub(crate) fn target_number(
    target: &NotifyTarget,
    connector: &str,
    read: fn(&str) -> Result<String>,
) -> Result<String> {
    match target.target_type {
        NotifyTargetType::DirectUser | NotifyTargetType::FixedAddress => read(&target.value),
        NotifyTargetType::FixedGroup | NotifyTargetType::Webhook => Err(Error::invalid(format!(
            "{connector} target_type must be direct_user or fixed_address"
        ))),
    }
}

/// 电话里念的话：标题，再是正文；Markdown 的记号与标题里的「·」不念。
pub(crate) fn spoken(message: &NotifyMessage) -> String {
    let title = plain(&message.title).replace(" · ", ", ");
    let text = plain(&message.text);

    let said = if text.is_empty() || text == title {
        title
    } else if title.is_empty() {
        text
    } else {
        format!("{}. {text}", title.trim_end_matches(['.', '。']))
    };

    super::truncate_chars(said.trim(), MAX_SPOKEN_CHARS)
}

/// 短信的内容：标题一行，正文一行。
pub(crate) fn texted(message: &NotifyMessage) -> String {
    let title = message.title.trim();
    let text = plain(&message.text);

    let body = if text.is_empty() || text == title {
        title.to_string()
    } else {
        format!("{title}\n{text}")
    };

    super::truncate_chars(body.trim(), MAX_TEXT_CHARS)
}

/// 去掉 Markdown 的粗体、行内代码与标题记号，换行并成空格：念出来、填进模板变量都用这一行。
pub(crate) fn plain(value: &str) -> String {
    value
        .lines()
        .map(|line| line.trim().trim_start_matches(['#', '>']).trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .replace("**", "")
        .replace('`', "")
}

/// 配置里开了「回复」：发出的短信收回复代码，电话收按键。
pub(super) fn replies_on(config: &serde_json::Value) -> bool {
    config
        .get("replies")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn message(title: &str, text: &str) -> NotifyMessage {
        NotifyMessage {
            title: title.into(),
            text: text.into(),
            markdown: None,
            html: None,
            metadata: BTreeMap::new(),
            provider_templates: Default::default(),
            event: None,
            card: None,
        }
    }

    #[test]
    fn numbers_are_read_as_e164() {
        assert_eq!(e164("+86 138-0013-8000").unwrap(), "+8613800138000");
        assert_eq!(e164("0044 (20) 7946.0958").unwrap(), "+442079460958");
        assert!(e164("13800138000").is_err());
        assert!(e164("+0123456789").is_err());
        assert!(e164("+86abc").is_err());
        assert!(e164("+1234567").is_err());
    }

    #[test]
    fn alibaba_cloud_also_reads_a_bare_mainland_mobile_number() {
        assert_eq!(
            e164_or_mainland_mobile("13800138000").unwrap(),
            "+8613800138000"
        );
        assert_eq!(
            e164_or_mainland_mobile("+12025550123").unwrap(),
            "+12025550123"
        );
        // 不是手机号的 11 位数字不当内地号码。
        assert!(e164_or_mainland_mobile("12025550123").is_err());
    }

    #[test]
    fn a_mainland_call_dials_the_national_number() {
        assert_eq!(mainland_dial("+8613800138000").unwrap(), "13800138000");
        assert_eq!(mainland_dial("+861088886666").unwrap(), "01088886666");
        assert_eq!(mainland_dial("+8657188886666").unwrap(), "057188886666");
        assert!(mainland_dial("+12025550123").is_err());
    }

    #[test]
    fn a_call_reads_the_title_then_the_text() {
        let said = spoken(&message(
            "CRITICAL · Checkout errors",
            "Alert **Checkout errors** is `open`.",
        ));

        assert_eq!(
            said,
            "CRITICAL, Checkout errors. Alert Checkout errors is open."
        );
        assert_eq!(spoken(&message("Disk full", "Disk full")), "Disk full");
        assert!(spoken(&message(&"x".repeat(900), "")).chars().count() <= MAX_SPOKEN_CHARS);
    }

    #[test]
    fn a_text_has_the_title_on_its_own_line() {
        assert_eq!(
            texted(&message(
                "CRITICAL · Checkout errors",
                "## Errors rose\n5xx at 12%"
            )),
            "CRITICAL · Checkout errors\nErrors rose 5xx at 12%"
        );
        assert!(texted(&message("t", &"y".repeat(900))).chars().count() <= MAX_TEXT_CHARS);
    }

    #[test]
    fn only_a_person_or_a_fixed_number_is_called() {
        let target = |target_type| NotifyTarget {
            target_type,
            value: "+8613800138000".into(),
            metadata: BTreeMap::new(),
        };

        assert!(target_number(&target(NotifyTargetType::DirectUser), "x", e164).is_ok());
        assert!(target_number(&target(NotifyTargetType::FixedAddress), "x", e164).is_ok());
        assert!(target_number(&target(NotifyTargetType::FixedGroup), "x", e164).is_err());
    }
}
