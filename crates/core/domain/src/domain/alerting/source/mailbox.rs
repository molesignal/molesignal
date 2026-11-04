// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 邮件来源读的 IMAP 邮箱。
//!
//! 自托管的产品选 IMAP 轮询：只要连得出去、有一个邮箱，不用公网地址、MX 记录、25 端口和证书。只读
//! （`EXAMINE` + `BODY.PEEK`），不改邮件的已读状态，也不删；读到哪了记在 [`MailCursor`]，第一次连上时从
//! 当时的位置开始，不吃存量邮件。
//!
//! 邮箱只配在来源的接收集群上，不随联邦同步：密码不出集群。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

/// 默认多久读一次。
pub const DEFAULT_MAILBOX_POLL_SECS: u32 = 60;
/// 最短多久读一次：邮件服务器常按频率限制登录。
pub const MIN_MAILBOX_POLL_SECS: u32 = 15;
/// 最长多久读一次。
pub const MAX_MAILBOX_POLL_SECS: u32 = 3600;

fn default_poll_secs() -> u32 {
    DEFAULT_MAILBOX_POLL_SECS
}

fn inbox() -> String {
    "INBOX".into()
}

fn imaps_port() -> u16 {
    993
}

/// 连邮箱怎么加密。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailboxSecurity {
    /// 一连上就是 TLS（通常是 993 端口）。
    #[default]
    Tls,
    /// 明文连上再升级（通常是 143 端口）。
    Starttls,
    /// 不加密：密码明文过网，只该用在可信的内网。
    None,
}

/// 登录邮箱的方式。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum MailboxAuth {
    /// 用户名和密码（或邮箱服务给的授权码、应用专用密码）。
    #[default]
    Password,
    /// OAuth 2.0 客户端凭证换来的令牌，走 `XOAUTH2`：Microsoft 365 已经不收密码登录。秘密是客户端密钥。
    Oauth2 {
        token_url: String,
        client_id: String,
        scope: String,
    },
}

/// 邮箱的连接设置，不含秘密。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailboxSettings {
    pub host: String,
    #[serde(default = "imaps_port")]
    pub port: u16,
    #[serde(default)]
    pub security: MailboxSecurity,
    pub username: String,
    #[serde(default)]
    pub auth: MailboxAuth,
    #[serde(default = "inbox")]
    pub folder: String,
    #[serde(default = "default_poll_secs")]
    pub poll_secs: u32,
}

impl MailboxSettings {
    /// 收拾空白，再看设置说不说得通。
    pub fn validate(&mut self) -> Result<()> {
        self.host = self.host.trim().to_string();
        self.username = self.username.trim().to_string();
        self.folder = self.folder.trim().to_string();

        if self.host.is_empty() || self.host.chars().any(char::is_whitespace) {
            return Err(Error::invalid("the mailbox needs a host name"));
        }

        if self.host.contains('/') || self.host.contains(':') {
            return Err(Error::invalid(
                "write the host name alone, without a scheme or a port",
            ));
        }

        if self.port == 0 {
            return Err(Error::invalid("the mailbox needs a port"));
        }

        if self.username.is_empty() {
            return Err(Error::invalid("the mailbox needs a user name"));
        }

        if self.folder.is_empty() {
            self.folder = inbox();
        }

        // 文件夹名会写进 IMAP 命令。
        if self.folder.chars().any(char::is_control) {
            return Err(Error::invalid("the folder name has control characters"));
        }

        if !(MIN_MAILBOX_POLL_SECS..=MAX_MAILBOX_POLL_SECS).contains(&self.poll_secs) {
            return Err(Error::invalid(format!(
                "poll_secs is between {MIN_MAILBOX_POLL_SECS} and {MAX_MAILBOX_POLL_SECS}"
            )));
        }

        if let MailboxAuth::Oauth2 {
            token_url,
            client_id,
            scope,
        } = &mut self.auth
        {
            *token_url = token_url.trim().to_string();
            *client_id = client_id.trim().to_string();
            *scope = scope.trim().to_string();

            if !token_url.to_ascii_lowercase().starts_with("https://") {
                return Err(Error::invalid("the token address starts with https://"));
            }

            if client_id.is_empty() {
                return Err(Error::invalid("OAuth 2.0 needs the client ID"));
            }
        }

        Ok(())
    }

    /// 换了地方（主机、账号、文件夹）：读到哪了要重新算。
    pub fn same_place(&self, other: &Self) -> bool {
        self.host.eq_ignore_ascii_case(&other.host)
            && self.username == other.username
            && self.folder == other.folder
    }
}

/// 读到哪了：`UIDVALIDITY` 变了说明 UID 重排了，要重新从当时的位置开始。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailCursor {
    pub uid_validity: u32,
    pub last_uid: u32,
}

/// 一个来源的邮箱。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mailbox {
    pub source_id: Id,
    pub org_id: Id,
    pub settings: MailboxSettings,
    /// 密码或客户端密钥。读出来给页面时不带。
    #[serde(default, skip_serializing)]
    pub secret: String,
    #[serde(default)]
    pub cursor: Option<MailCursor>,
    #[serde(default)]
    pub last_polled_at: Option<TimestampMicros>,
    #[serde(default)]
    pub last_success_at: Option<TimestampMicros>,
    /// 上一次读失败的原因；读成功就清掉。
    #[serde(default)]
    pub last_error: String,
    pub updated_at: TimestampMicros,
}

impl Mailbox {
    /// 该读了。
    pub fn due(&self, now: TimestampMicros) -> bool {
        self.last_polled_at.is_none_or(|at| {
            now.0.saturating_sub(at.0) >= i64::from(self.settings.poll_secs) * 1_000_000
        })
    }
}

#[async_trait]
pub trait MailboxRepository: Send + Sync {
    async fn get(&self, source_id: &Id) -> Result<Option<Mailbox>>;
    /// 所有邮箱，带着秘密：本集群读邮箱的任务用。
    async fn list_all(&self) -> Result<Vec<Mailbox>>;
    /// 写设置与秘密；读到哪了和上次的结果也一起写，换了地方时由调用方清掉。
    async fn save(&self, mailbox: Mailbox) -> Result<Mailbox>;
    async fn delete(&self, source_id: &Id) -> Result<()>;
    /// 记下读到哪了。
    async fn advance(&self, source_id: &Id, cursor: MailCursor) -> Result<()>;
    /// 记下这一次读的结果：`error` 空是成功。
    async fn record_poll(
        &self,
        source_id: &Id,
        at: TimestampMicros,
        error: Option<&str>,
    ) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> MailboxSettings {
        MailboxSettings {
            host: " imap.example.com ".into(),
            port: 993,
            security: MailboxSecurity::Tls,
            username: "alerts@example.com".into(),
            auth: MailboxAuth::Password,
            folder: String::new(),
            poll_secs: 60,
        }
    }

    #[test]
    fn settings_are_tidied_and_checked() {
        let mut fine = settings();
        fine.validate().unwrap();
        assert_eq!(fine.host, "imap.example.com");
        assert_eq!(fine.folder, "INBOX");

        let mut scheme = settings();
        scheme.host = "imaps://imap.example.com".into();
        assert!(scheme.validate().is_err());

        let mut fast = settings();
        fast.poll_secs = 1;
        assert!(fast.validate().is_err());

        let mut no_user = settings();
        no_user.username = " ".into();
        assert!(no_user.validate().is_err());

        let mut plain_token = settings();
        plain_token.auth = MailboxAuth::Oauth2 {
            token_url: "http://login.example.com/token".into(),
            client_id: "c".into(),
            scope: "s".into(),
        };
        assert!(plain_token.validate().is_err());
    }

    #[test]
    fn saved_settings_without_the_optional_fields_take_the_defaults() {
        let parsed: MailboxSettings = serde_json::from_value(serde_json::json!({
            "host": "imap.example.com", "username": "u"
        }))
        .unwrap();

        assert_eq!(parsed.port, 993);
        assert_eq!(parsed.security, MailboxSecurity::Tls);
        assert_eq!(parsed.auth, MailboxAuth::Password);
        assert_eq!(parsed.folder, "INBOX");
        assert_eq!(parsed.poll_secs, DEFAULT_MAILBOX_POLL_SECS);
    }

    #[test]
    fn a_mailbox_is_due_after_its_interval() {
        let mailbox = Mailbox {
            source_id: Id::from_string("s"),
            org_id: Id::from_string("o"),
            settings: settings(),
            secret: "p".into(),
            cursor: None,
            last_polled_at: Some(TimestampMicros(1_000_000)),
            last_success_at: None,
            last_error: String::new(),
            updated_at: TimestampMicros(1),
        };

        assert!(!mailbox.due(TimestampMicros(60_000_000)));
        assert!(mailbox.due(TimestampMicros(61_000_000)));
    }

    #[test]
    fn the_secret_stays_out_of_what_is_serialized() {
        let mailbox = Mailbox {
            source_id: Id::from_string("s"),
            org_id: Id::from_string("o"),
            settings: settings(),
            secret: "p".into(),
            cursor: None,
            last_polled_at: None,
            last_success_at: None,
            last_error: String::new(),
            updated_at: TimestampMicros(1),
        };

        assert!(
            serde_json::to_value(&mailbox)
                .unwrap()
                .get("secret")
                .is_none()
        );
    }
}
