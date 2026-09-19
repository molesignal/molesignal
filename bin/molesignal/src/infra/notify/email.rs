// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! SMTP 邮件 sender。
//!
//! `lettre` 的 `SmtpTransport` 是同步的，我们用 `tokio::task::spawn_blocking` 包一层。
//! TLS 由 `[notify.smtp].tls`（`none` / `starttls` / `tls`）决定。

use lettre::{
    Message, SmtpTransport, Transport,
    message::header::ContentType,
    transport::smtp::{
        authentication::Credentials,
        client::{Tls, TlsParameters},
    },
};

use crate::{
    config::{SmtpSettings, SmtpTls},
    shared::{Error, Result},
};

pub struct EmailSender {
    transport: SmtpTransport,
    from: String,
}

impl EmailSender {
    pub fn new(cfg: &SmtpSettings) -> Result<Self> {
        if cfg.host.is_empty() {
            return Err(Error::invalid("notify.smtp.host is empty"));
        }
        if cfg.from.is_empty() {
            return Err(Error::invalid("notify.smtp.from is empty"));
        }

        let port = if cfg.port == 0 { 587 } else { cfg.port };

        let mut builder = match cfg.tls {
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

        if !cfg.username.is_empty() {
            builder =
                builder.credentials(Credentials::new(cfg.username.clone(), cfg.password.clone()));
        }

        builder = builder.timeout(Some(std::time::Duration::from_secs(
            cfg.timeout_secs.max(1) as u64,
        )));

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
