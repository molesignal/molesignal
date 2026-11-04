// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 多因素认证有了变化时告诉账号主人（邮件），并记进审计。
//!
//! 邮件用账号主人读界面的语言：本人的设置，没设时用当时所在组织的默认语言。没配 SMTP 时不发，
//! 发送失败只记日志，不挡住操作本身。

use crate::{
    api::AppState,
    domain::{
        i18n_language::{FALLBACK_LANGUAGE_CODE, LanguagePack},
        iam::User,
    },
    infra::persistence::repositories::audit_events::AuditEvent,
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// 邮件主题与正文在 `i18n_language` 表里的键前缀。
const EMAIL_KEY: &str = "auth.email.mfa";

/// 要告诉账号主人的事。
pub(super) enum Notice<'a> {
    Enabled,
    Disabled,
    /// 管理员重置了多因素认证。
    Reset {
        admin: &'a str,
    },
    /// 有人用恢复码登录，还剩 `left` 个。
    RecoveryCodeUsed {
        left: i64,
    },
    /// 验证码输错太多次，锁了 `minutes` 分钟。
    Locked {
        minutes: i64,
    },
    /// 账号添加了一个 Passkey。
    PasskeyAdded,
}

impl Notice<'_> {
    fn key(&self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
            Self::Reset { .. } => "reset",
            Self::RecoveryCodeUsed { .. } => "recovery_code_used",
            Self::Locked { .. } => "locked",
            Self::PasskeyAdded => "passkey_added",
        }
    }

    fn fills(&self, email: &str) -> Vec<(&'static str, String)> {
        let mut fills = vec![("email", email.to_owned())];

        match self {
            Self::Enabled | Self::Disabled | Self::PasskeyAdded => {}
            Self::Reset { admin } => fills.push(("admin", (*admin).to_owned())),
            Self::RecoveryCodeUsed { left } => fills.push(("left", left.to_string())),
            Self::Locked { minutes } => fills.push(("minutes", minutes.to_string())),
        }

        fills
    }
}

/// 发邮件告诉账号主人。
pub(super) async fn tell_owner(state: &AppState, user: &User, org_id: &Id, notice: Notice<'_>) {
    let Some(sender) = state.iam.email_sender.clone() else {
        return;
    };

    let language = owner_language(state, &user.id, org_id).await;

    let copy = match state
        .platform
        .i18n_language
        .load_pack(EMAIL_KEY, &language)
        .await
    {
        Ok(copy) => copy,
        Err(error) => {
            tracing::warn!(error = %error, "mfa notice: email copy could not be read");
            return;
        }
    };

    let (subject, body) = match written(&copy, &notice, &user.email) {
        Ok(email) => email,
        Err(error) => {
            tracing::warn!(error = %error, "mfa notice: email could not be written");
            return;
        }
    };

    let to = user.email.clone();

    crate::shared::trace_context::spawn_with_current_trace_context(async move {
        if let Err(error) = sender.send_text(&[to], &subject, &body).await {
            tracing::warn!(error = %error, "mfa notice: email send failed");
        }
    });
}

fn written(copy: &LanguagePack, notice: &Notice<'_>, email: &str) -> Result<(String, String)> {
    let fills = notice.fills(email);

    let fills = fills
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect::<Vec<_>>();

    let key = notice.key();

    Ok((
        copy.render(&format!("{EMAIL_KEY}.{key}.subject"), &fills)?,
        copy.render(&format!("{EMAIL_KEY}.{key}.body"), &fills)?,
    ))
}

/// 账号主人读界面的语言：本人的设置，没设时用组织的默认语言。
async fn owner_language(state: &AppState, user_id: &Id, org_id: &Id) -> String {
    match state.iam.user_preferences.get_optional(user_id).await {
        Ok(Some(personal)) => return personal.language,
        Ok(None) => {}
        Err(error) => tracing::warn!(error = %error, "mfa notice: personal language unreadable"),
    }

    match state.iam.workspace_preference_defaults.get(org_id).await {
        Ok(workspace) => workspace.language,
        Err(error) => {
            tracing::warn!(error = %error, "mfa notice: workspace language unreadable");
            FALLBACK_LANGUAGE_CODE.to_owned()
        }
    }
}

/// 记一条审计：`actor_id` 在组织 `org_id` 里做了 `action`，对象是账号 `target`。
pub(super) async fn audit(
    state: &AppState,
    org_id: &Id,
    actor_id: &Id,
    action: &str,
    target: &Id,
    payload: serde_json::Value,
) {
    let recorded = state
        .iam
        .audit_events
        .record(AuditEvent {
            id: Id::new(),
            org_id: org_id.clone(),
            actor_kind: "user".into(),
            actor_id: actor_id.0.clone(),
            action: action.into(),
            target_kind: Some("user".into()),
            target_id: Some(target.0.clone()),
            ip: None,
            user_agent: None,
            payload,
            ts: TimestampMicros::now(),
        })
        .await;

    if let Err(error) = recorded {
        tracing::warn!(error = %error, action, "mfa audit event could not be recorded");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::i18n_language_seed::seeded_pack;

    #[test]
    fn every_notice_is_written_in_both_languages() {
        let notices = [
            Notice::Enabled,
            Notice::Disabled,
            Notice::Reset { admin: "Ada" },
            Notice::RecoveryCodeUsed { left: 3 },
            Notice::Locked { minutes: 60 },
        ];

        for language in ["en-us", "zh-cn"] {
            let copy = seeded_pack(EMAIL_KEY, language);

            for notice in &notices {
                let (subject, body) = written(&copy, notice, "alice@example.com").unwrap();
                assert!(subject.starts_with("MoleSignal"));
                assert!(body.contains("alice@example.com"));
            }
        }

        let copy = seeded_pack(EMAIL_KEY, "zh-cn");
        let (_, used) = written(&copy, &Notice::RecoveryCodeUsed { left: 3 }, "a@b.c").unwrap();
        assert!(used.contains("剩余恢复码：3 个"));

        let (_, reset) = written(&copy, &Notice::Reset { admin: "Ada" }, "a@b.c").unwrap();
        assert!(reset.starts_with("Ada 重置了"));
    }
}
