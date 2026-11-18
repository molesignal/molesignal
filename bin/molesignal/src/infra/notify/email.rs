// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! SMTP 邮件 sender。
//!
//! `lettre` 的 `SmtpTransport` 是同步的，我们用 `tokio::task::spawn_blocking` 包一层。
//! TLS 由 `[notify.smtp].tls`（`none` / `starttls` / `tls`）决定。

use kernel::{Error, Result};
use lettre::{
    Message, SmtpTransport, Transport,
    message::header::ContentType,
    transport::smtp::{
        SmtpTransportBuilder,
        authentication::{Credentials, Mechanism},
        client::{Tls, TlsParameters},
    },
};
use settings::{SmtpSettings, SmtpTls};

pub struct EmailSender {
    transport: SmtpTransport,
    from: String,
}

impl EmailSender {
    pub fn new(cfg: &SmtpSettings) -> Result<Self> {
        let mut builder = transport(cfg)?;

        if !cfg.username.is_empty() {
            builder =
                builder.credentials(Credentials::new(cfg.username.clone(), cfg.password.clone()));
        }

        Ok(Self {
            transport: builder.build(),
            from: cfg.from.clone(),
        })
    }

    /// Signs in with an OAuth 2.0 access token (SASL XOAUTH2) as `cfg.username`, the way
    /// Microsoft 365 and Gmail ask applications to; `cfg.password` is not used.
    pub fn with_xoauth2(cfg: &SmtpSettings, access_token: &str) -> Result<Self> {
        let username = cfg.username.trim();

        if username.is_empty() {
            return Err(Error::invalid(
                "OAuth sign-in needs the sending mailbox as the SMTP username",
            ));
        }

        let builder = transport(cfg)?
            .credentials(Credentials::new(
                username.to_string(),
                access_token.to_string(),
            ))
            .authentication(vec![Mechanism::Xoauth2]);

        Ok(Self {
            transport: builder.build(),
            from: cfg.from.clone(),
        })
    }

    /// 发送一封纯文本邮件（通用：注册审批通知等非告警场景复用）。
    pub async fn send_text(&self, to: &[String], subject: &str, body: &str) -> Result<()> {
        self.send_message(to, &[], &[], None, subject, body, false)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn send_message(
        &self,
        to: &[String],
        cc: &[String],
        bcc: &[String],
        reply_to: Option<&str>,
        subject: &str,
        body: &str,
        html: bool,
    ) -> Result<()> {
        self.send_with_content_type(
            to,
            cc,
            bcc,
            reply_to,
            subject,
            body,
            if html {
                ContentType::TEXT_HTML
            } else {
                ContentType::TEXT_PLAIN
            },
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn send_with_content_type(
        &self,
        to: &[String],
        cc: &[String],
        bcc: &[String],
        reply_to: Option<&str>,
        subject: &str,
        body: &str,
        content_type: ContentType,
    ) -> Result<()> {
        if to.is_empty() {
            return Err(Error::invalid("email has empty `to` list"));
        }

        let from = self.from.parse().map_err(|e| {
            Error::internal(format!("smtp from address parse '{}': {e}", self.from))
        })?;

        let mut builder = Message::builder().from(from).subject(subject.to_string());

        for addr in to {
            let parsed = addr
                .parse()
                .map_err(|e| Error::invalid(format!("smtp to '{addr}': {e}")))?;

            builder = builder.to(parsed);
        }

        for addr in cc {
            let parsed = addr
                .parse()
                .map_err(|e| Error::invalid(format!("smtp cc '{addr}': {e}")))?;

            builder = builder.cc(parsed);
        }

        for addr in bcc {
            let parsed = addr
                .parse()
                .map_err(|e| Error::invalid(format!("smtp bcc '{addr}': {e}")))?;

            builder = builder.bcc(parsed);
        }

        if let Some(addr) = reply_to.filter(|addr| !addr.trim().is_empty()) {
            let parsed = addr
                .parse()
                .map_err(|e| Error::invalid(format!("smtp reply-to '{addr}': {e}")))?;

            builder = builder.reply_to(parsed);
        }

        let msg = builder
            .header(content_type)
            .body(body.to_string())
            .map_err(|e| Error::internal(format!("smtp build: {e}")))?;

        let transport = self.transport.clone();

        tokio::task::spawn_blocking(move || transport.send(&msg))
            .await
            .map_err(|e| Error::internal(format!("smtp blocking task: {e}")))?
            .map_err(|e| Error::internal(format!("smtp send: {e}")))?;

        Ok(())
    }
}

/// The transport to `cfg.host` with its TLS mode and timeout; the caller adds the credentials.
fn transport(cfg: &SmtpSettings) -> Result<SmtpTransportBuilder> {
    if cfg.host.is_empty() {
        return Err(Error::invalid("notify.smtp.host is empty"));
    }

    if cfg.from.is_empty() {
        return Err(Error::invalid("notify.smtp.from is empty"));
    }

    let port = if cfg.port == 0 { 587 } else { cfg.port };

    let builder = match cfg.tls {
        SmtpTls::None => SmtpTransport::builder_dangerous(&cfg.host).port(port),
        SmtpTls::Starttls => SmtpTransport::starttls_relay(&cfg.host)
            .map_err(|e| Error::internal(format!("smtp starttls: {e}")))?
            .port(port),
        SmtpTls::Tls => {
            let params = TlsParameters::new(cfg.host.clone())
                .map_err(|e| Error::internal(format!("smtp tls params: {e}")))?;

            SmtpTransport::relay(&cfg.host)
                .map_err(|e| Error::internal(format!("smtp relay: {e}")))?
                .port(port)
                .tls(Tls::Wrapper(params))
        }
    };

    Ok(builder.timeout(Some(std::time::Duration::from_secs(
        cfg.timeout_secs.max(1) as u64,
    ))))
}
