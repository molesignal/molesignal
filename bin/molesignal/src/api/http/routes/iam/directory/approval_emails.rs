// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 自助注册审批的邮件通知：有人待审批时通知管理员，审批有结果时通知本人。
//!
//! 全部 best-effort：没有配置 SMTP、文案取不到或发送失败都只记日志，绝不阻塞注册和审批。
//! 主题与正文在 `i18n_language` 表里，按工作区的默认语言取。

use crate::{
    api::AppState,
    app::iam::IamSubject,
    domain::{
        i18n_language::{FALLBACK_LANGUAGE_CODE, LanguagePack},
        iam::IamScope,
    },
    shared::ids::Id,
};

/// 审批邮件在 `i18n_language` 表里的键前缀。
const KEY_PREFIX: &str = "iam.email.";

/// 通知 org 管理员有新用户待审批。
pub(crate) async fn notify_admins_pending_user(state: &AppState, org_id: &Id, pending_email: &str) {
    let Some(sender) = state.iam.email_sender.as_ref() else {
        return;
    };

    let admins = collect_org_admin_emails(state, org_id).await;

    if admins.is_empty() {
        return;
    }

    let Some(copy) = approval_copy(state, org_id).await else {
        return;
    };

    let base = state.platform.external_url.trim_end_matches('/');
    let review_url = format!("{base}/iam/approvals");

    let fills = [
        ("pending_email", pending_email),
        ("review_url", review_url.as_str()),
    ];

    let Some((subject, body)) = written(&copy, "registration_pending", !base.is_empty(), &fills)
    else {
        return;
    };

    if let Err(e) = sender.send_text(&admins, &subject, &body).await {
        tracing::warn!(error = %e, "approval-notify: send to admins failed");
    }
}

/// 通知用户审批结果。
pub(super) async fn notify_user_decision(
    state: &AppState,
    org_id: &Id,
    email: &str,
    approved: bool,
) {
    let Some(sender) = state.iam.email_sender.as_ref() else {
        return;
    };

    let Some(copy) = approval_copy(state, org_id).await else {
        return;
    };

    let base = state.platform.external_url.trim_end_matches('/');
    let signin_url = format!("{base}/signin");
    let fills = [("signin_url", signin_url.as_str())];

    // 只有“已通过”的正文带登录链接；“未通过”没有不带链接的版本。
    let decision = if approved {
        written(&copy, "registration_approved", !base.is_empty(), &fills)
    } else {
        written(&copy, "registration_declined", true, &fills)
    };

    let Some((subject, body)) = decision else {
        return;
    };

    if let Err(e) = sender
        .send_text(&[email.to_string()], &subject, &body)
        .await
    {
        tracing::warn!(error = %e, approved, "approval-notify: send to user failed");
    }
}

/// 收集拥有成员管理能力的邮箱（审批通知收件人）。
async fn collect_org_admin_emails(state: &AppState, org_id: &Id) -> Vec<String> {
    let members = match state.iam.service.iam_memberships.list_for_org(org_id).await {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(error = %e, "approval-notify: list org members failed");
            return Vec::new();
        }
    };

    let mut emails = Vec::new();

    for m in members {
        let can_manage_members = state
            .iam
            .access
            .capabilities(&IamSubject {
                user_id: m.user_id.clone(),
                organization_id: org_id.clone(),
                credential_role_id: None,
                credential_application_id: None,
                credential_service_account_id: None,
                scope: IamScope::Organization,
            })
            .await
            .is_ok_and(|capabilities| capabilities.has("org.members.manage"));

        if can_manage_members && let Ok(u) = state.iam.service.users.get(&m.user_id).await {
            emails.push(u.email);
        }
    }

    emails
}

/// 审批邮件的文案，按工作区的默认语言取；取不到语言时用回退语言。
async fn approval_copy(state: &AppState, org_id: &Id) -> Option<LanguagePack> {
    let language = match state.iam.workspace_preference_defaults.get(org_id).await {
        Ok(defaults) => defaults.language,
        Err(error) => {
            tracing::warn!(error = %error, "approval-notify: workspace language lookup failed");
            FALLBACK_LANGUAGE_CODE.to_string()
        }
    };

    match state
        .platform
        .i18n_language
        .load_pack(KEY_PREFIX, &language)
        .await
    {
        Ok(copy) => Some(copy),
        Err(error) => {
            tracing::warn!(error = %error, "approval-notify: email copy could not be read");
            None
        }
    }
}

/// 一封邮件的主题与正文。`with_link` 为假时取不带链接的正文：`http.external_url` 为空时
/// 没有可用的链接。
fn written(
    copy: &LanguagePack,
    email: &str,
    with_link: bool,
    fills: &[(&str, &str)],
) -> Option<(String, String)> {
    let body = if with_link {
        "body"
    } else {
        "body_without_link"
    };

    let subject = copy.render(&format!("{KEY_PREFIX}{email}.subject"), fills);
    let body = copy.render(&format!("{KEY_PREFIX}{email}.{body}"), fills);

    match (subject, body) {
        (Ok(subject), Ok(body)) => Some((subject, body)),
        (Err(error), _) | (_, Err(error)) => {
            tracing::warn!(error = %error, "approval-notify: email could not be written");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::i18n_language_seed::seeded_pack;

    #[test]
    fn pending_registration_email_points_admins_to_the_approvals() {
        let fills = [
            ("pending_email", "new@acme.example"),
            ("review_url", "https://molesignal.example/iam/approvals"),
        ];

        let (subject, body) = written(
            &seeded_pack(KEY_PREFIX, "en-us"),
            "registration_pending",
            true,
            &fills,
        )
        .unwrap();
        assert_eq!(subject, "MoleSignal: a new user is awaiting approval");
        assert_eq!(
            body,
            "A new user has signed up and is awaiting your approval:\n\n  new@acme.example\n\nReview: https://molesignal.example/iam/approvals\n"
        );

        let (subject, body) = written(
            &seeded_pack(KEY_PREFIX, "zh-cn"),
            "registration_pending",
            false,
            &fills,
        )
        .unwrap();
        assert_eq!(subject, "MoleSignal：有新用户等待审批");
        assert_eq!(
            body,
            "有新用户完成注册，正在等待审批：\n\n  new@acme.example\n\n请在 IAM → 审批 中处理待审批的注册。\n"
        );
    }

    #[test]
    fn decision_emails_exist_in_both_workspace_languages() {
        let fills = [("signin_url", "https://molesignal.example/signin")];

        let english = seeded_pack(KEY_PREFIX, "en-us");
        let chinese = seeded_pack(KEY_PREFIX, "zh-cn");

        let (subject, body) = written(&english, "registration_approved", true, &fills).unwrap();
        assert_eq!(subject, "MoleSignal: your account has been approved");
        assert_eq!(
            body,
            "Good news \u{2014} your registration has been approved. Sign in: https://molesignal.example/signin\n"
        );

        let (_, body) = written(&english, "registration_approved", false, &fills).unwrap();
        assert_eq!(
            body,
            "Good news \u{2014} your registration has been approved. You can now sign in.\n"
        );

        let (subject, body) = written(&chinese, "registration_approved", true, &fills).unwrap();
        assert_eq!(subject, "MoleSignal：你的账号已通过审批");
        assert_eq!(
            body,
            "你的注册申请已通过审批。登录地址：https://molesignal.example/signin\n"
        );

        let (subject, body) = written(&chinese, "registration_declined", true, &fills).unwrap();
        assert_eq!(subject, "MoleSignal：你的注册申请未通过");
        assert_eq!(body, "管理员没有通过你的注册申请。\n");

        let (subject, _) = written(&english, "registration_declined", true, &fills).unwrap();
        assert_eq!(subject, "MoleSignal: your registration was declined");
    }

    #[test]
    fn an_email_missing_from_the_table_is_not_sent() {
        assert!(written(&LanguagePack::default(), "registration_pending", true, &[]).is_none());
    }
}
