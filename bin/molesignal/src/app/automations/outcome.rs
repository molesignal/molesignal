// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一次运行怎么结束：Mole Agent 那一轮的结局，加上没人可问时被挡住的地方，再加上输出的结果。
//! 结束后发的通知也在这里写：文案来自 `i18n_language`，用规则保存时的语言。

use std::collections::BTreeMap;

use crate::{
    agent::{
        automations::{AutomationRunStatus, reasons},
        runs::RunStatus,
    },
    domain::{i18n_language::LanguagePack, notify::connector::NotifyMessage},
    shared::Result,
};

/// 通知文案与原因说明在 `i18n_language` 里的键前缀。
pub const KEY_PREFIX: &str = "automation.";
/// 通知里回答的开头最长这么多字符。
const SUMMARY_CHARS: usize = 1_500;
/// 运行记录里回答的开头最长这么多字符。
pub const RECORDED_SUMMARY_CHARS: usize = 600;

/// Mole Agent 那一轮的结局。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnswerEnding {
    pub status: RunStatus,
    /// 没开始就被拒绝时的固定代码，例如预算用完。
    pub code: Option<String>,
    /// 超过了规则的总时限，被停下。
    pub timed_out: bool,
}

/// 运行的状态与原因。`blocked` 是没人可问时被挡住的地方（提问、确认、缺凭据）。
pub fn status_of(
    ending: &AnswerEnding,
    blocked: &[&'static str],
) -> (AutomationRunStatus, Vec<&'static str>) {
    let mut blocked = blocked.to_vec();
    blocked.dedup();

    match ending.status {
        _ if ending.timed_out => (AutomationRunStatus::Failed, vec![reasons::TIMEOUT]),
        RunStatus::Succeeded if blocked.is_empty() => (AutomationRunStatus::Succeeded, Vec::new()),
        RunStatus::Succeeded => (AutomationRunStatus::Blocked, blocked),
        RunStatus::Failed => match ending.code.as_deref() {
            Some(reasons::BUDGET_EXHAUSTED) => (
                AutomationRunStatus::Skipped,
                vec![reasons::BUDGET_EXHAUSTED],
            ),
            Some(reasons::RUN_AS_UNAVAILABLE) => (
                AutomationRunStatus::Failed,
                vec![reasons::RUN_AS_UNAVAILABLE],
            ),
            _ => (AutomationRunStatus::Failed, vec![reasons::ERROR]),
        },
        RunStatus::Cancelled => (AutomationRunStatus::Abandoned, vec![reasons::STOPPED]),
        _ => (AutomationRunStatus::Abandoned, vec![reasons::INTERRUPTED]),
    }
}

/// 输出之后：有通知没发出去或 RCA 没写成时，成功改为部分成功，并记下原因。
pub fn after_outputs(
    (status, mut found): (AutomationRunStatus, Vec<&'static str>),
    delivery_failed: bool,
    rca_failed: bool,
) -> (AutomationRunStatus, Vec<&'static str>) {
    if delivery_failed {
        found.push(reasons::DELIVERY);
    }

    if rca_failed {
        found.push(reasons::RCA);
    }

    let status = match status {
        AutomationRunStatus::Succeeded if delivery_failed || rca_failed => {
            AutomationRunStatus::Partial
        }
        other => other,
    };

    (status, found)
}

/// 结束后要不要通知：做完了、被挡住了或失败了才通知；没开始的与被停下的不通知。
pub fn notifies(status: AutomationRunStatus) -> bool {
    matches!(
        status,
        AutomationRunStatus::Succeeded
            | AutomationRunStatus::Partial
            | AutomationRunStatus::Blocked
            | AutomationRunStatus::Failed
    )
}

/// 回答的开头：去掉首尾空白，最长 `chars` 个字符，截断处加省略号。
pub fn excerpt(content: &str, chars: usize) -> String {
    let content = content.trim();

    if content.chars().count() <= chars {
        return content.to_string();
    }

    let mut cut = content.chars().take(chars).collect::<String>();
    cut.push('…');
    cut
}

/// 结束后发到渠道的那条消息。
pub struct Notice<'a> {
    pub name: &'a str,
    pub status: AutomationRunStatus,
    pub reasons: &'a [&'static str],
    pub answer: Option<&'a str>,
    pub url: &'a str,
}

pub fn notification(pack: &LanguagePack, notice: &Notice<'_>) -> Result<NotifyMessage> {
    let title = pack.render(
        &format!("{KEY_PREFIX}notification.title.{}", notice.status.as_str()),
        &[("name", notice.name)],
    )?;

    let mut parts = Vec::new();

    if let Some(answer) = notice.answer.filter(|answer| !answer.trim().is_empty()) {
        parts.push(excerpt(answer, SUMMARY_CHARS));
    }

    for reason in notice.reasons {
        parts.push(pack.render(&format!("{KEY_PREFIX}reason.{reason}"), &[])?);
    }

    parts.push(pack.render(
        &format!("{KEY_PREFIX}notification.link"),
        &[("url", notice.url)],
    )?);

    Ok(NotifyMessage {
        title,
        text: parts.join("\n\n"),
        markdown: None,
        html: None,
        metadata: BTreeMap::from([("status".into(), notice.status.as_str().into())]),
        provider_templates: Default::default(),
        event: None,
        card: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::i18n_language_seed::seeded_pack;

    fn ending(status: RunStatus) -> AnswerEnding {
        AnswerEnding {
            status,
            code: None,
            timed_out: false,
        }
    }

    #[test]
    fn an_answer_that_needed_someone_is_blocked_and_a_budget_refusal_skipped() {
        assert_eq!(
            status_of(&ending(RunStatus::Succeeded), &[]),
            (AutomationRunStatus::Succeeded, vec![])
        );
        assert_eq!(
            status_of(
                &ending(RunStatus::Succeeded),
                &[reasons::CREDENTIAL, reasons::CREDENTIAL]
            ),
            (AutomationRunStatus::Blocked, vec![reasons::CREDENTIAL])
        );

        let budget = AnswerEnding {
            code: Some(reasons::BUDGET_EXHAUSTED.into()),
            ..ending(RunStatus::Failed)
        };
        assert_eq!(status_of(&budget, &[]).0, AutomationRunStatus::Skipped);

        let late = AnswerEnding {
            timed_out: true,
            ..ending(RunStatus::Cancelled)
        };
        assert_eq!(
            status_of(&late, &[]),
            (AutomationRunStatus::Failed, vec![reasons::TIMEOUT])
        );
        assert_eq!(
            status_of(&ending(RunStatus::Interrupted), &[]).0,
            AutomationRunStatus::Abandoned
        );
    }

    #[test]
    fn a_failed_delivery_turns_success_into_partial() {
        let done = (AutomationRunStatus::Succeeded, Vec::new());
        assert_eq!(
            after_outputs(done.clone(), true, false),
            (AutomationRunStatus::Partial, vec![reasons::DELIVERY])
        );
        assert_eq!(
            after_outputs(done, false, false).0,
            AutomationRunStatus::Succeeded
        );

        let blocked = (AutomationRunStatus::Blocked, vec![reasons::QUESTION]);
        assert_eq!(
            after_outputs(blocked, false, true),
            (
                AutomationRunStatus::Blocked,
                vec![reasons::QUESTION, reasons::RCA]
            )
        );
        assert!(!notifies(AutomationRunStatus::Skipped));
        assert!(notifies(AutomationRunStatus::Blocked));
    }

    #[test]
    fn the_notice_names_the_rule_says_why_and_links_the_run_in_both_languages() {
        for language in ["en-us", "zh-cn"] {
            let pack = seeded_pack(KEY_PREFIX, language);

            for status in AutomationRunStatus::ALL
                .into_iter()
                .filter(|s| notifies(*s))
            {
                let message = notification(
                    &pack,
                    &Notice {
                        name: "Daily health",
                        status,
                        reasons: &[reasons::CREDENTIAL],
                        answer: Some("All services are healthy."),
                        url: "https://molesignal.example/agent/chat?chat=c1",
                    },
                )
                .unwrap();

                assert!(
                    message.title.contains("Daily health"),
                    "{language} {status:?}"
                );
                assert!(message.text.contains("All services are healthy."));
                assert!(
                    message
                        .text
                        .contains("https://molesignal.example/agent/chat?chat=c1")
                );
            }

            for reason in [
                reasons::BUDGET_EXHAUSTED,
                reasons::STILL_RUNNING,
                reasons::ALREADY_ANALYZED,
                reasons::RUN_AS_UNAVAILABLE,
                reasons::NO_MODEL,
                reasons::TIMEOUT,
                reasons::ERROR,
                reasons::QUESTION,
                reasons::CONFIRMATION,
                reasons::CREDENTIAL,
                reasons::DELIVERY,
                reasons::RCA,
                reasons::STOPPED,
                reasons::INTERRUPTED,
            ] {
                assert!(
                    pack.get(&format!("{KEY_PREFIX}reason.{reason}")).is_some(),
                    "{language} {reason}"
                );
            }
        }
    }

    #[test]
    fn an_excerpt_is_cut_on_a_character() {
        assert_eq!(excerpt("  short  ", 10), "short");
        assert_eq!(excerpt("事件已恢复，原因是发布", 5), "事件已恢复…");
    }
}
