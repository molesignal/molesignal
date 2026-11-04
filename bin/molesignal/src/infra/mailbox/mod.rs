// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 读 IMAP 邮箱。只读：`EXAMINE` 选文件夹，`BODY.PEEK[]` 取邮件，不改已读状态，也不删。
//!
//! 连接只去主机名此刻解析到的地址，云厂商元数据服务的地址不去（同 Webhook）。证书按操作系统信任的算。

use std::{
    fmt, io,
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use async_imap::{Authenticator, Client, Session};
use async_trait::async_trait;
use futures::TryStreamExt;
use rustls::{
    ClientConfig,
    crypto::{CryptoProvider, aws_lc_rs},
    pki_types::ServerName,
};
use rustls_platform_verifier::Verifier;
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::TcpStream,
};
use tokio_rustls::{TlsConnector, client::TlsStream};

use crate::{
    app::alerting::sources::{FetchedMail, MailBatch, MailboxClient, MailboxStatus},
    domain::alerting::source::{
        MailCursor, Mailbox, MailboxAuth, MailboxSecurity, MailboxSettings,
    },
    infra::connectors::{config::http::is_metadata_address, dispatcher::http::pinned_client},
    shared::{Error, Result},
};

/// 连接和握手最多等这么久。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// 一次读邮箱最多等这么久。
const READ_TIMEOUT: Duration = Duration::from_secs(90);
/// 换 OAuth 令牌最多等这么久。
const TOKEN_TIMEOUT: Duration = Duration::from_secs(20);

/// 明文或 TLS 的连接：STARTTLS 要先明文再升级，读写两边用同一种类型。
#[derive(Debug)]
enum MailStream {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl AsyncRead for MailStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(stream) => Pin::new(stream).poll_read(cx, buf),
            Self::Tls(stream) => Pin::new(stream.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for MailStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Self::Plain(stream) => Pin::new(stream).poll_write(cx, buf),
            Self::Tls(stream) => Pin::new(stream.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(stream) => Pin::new(stream).poll_flush(cx),
            Self::Tls(stream) => Pin::new(stream.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(stream) => Pin::new(stream).poll_shutdown(cx),
            Self::Tls(stream) => Pin::new(stream.as_mut()).poll_shutdown(cx),
        }
    }
}

/// 经 IMAP 读邮箱。
#[derive(Debug, Default)]
pub struct ImapMailboxClient;

#[async_trait]
impl MailboxClient for ImapMailboxClient {
    async fn read_new(&self, mailbox: &Mailbox, limit: usize, max_bytes: u32) -> Result<MailBatch> {
        within_time(async {
            let mut session = open(&mailbox.settings, &mailbox.secret).await?;
            let batch = read(&mut session, mailbox, limit, max_bytes).await;
            let _ = session.logout().await;
            batch
        })
        .await
    }

    async fn check(&self, settings: &MailboxSettings, secret: &str) -> Result<MailboxStatus> {
        within_time(async {
            let mut session = open(settings, secret).await?;
            let folder = session.examine(&settings.folder).await.map_err(refused)?;
            let _ = session.logout().await;

            Ok(MailboxStatus {
                messages: folder.exists,
            })
        })
        .await
    }
}

async fn within_time<T>(work: impl std::future::Future<Output = Result<T>>) -> Result<T> {
    tokio::time::timeout(READ_TIMEOUT, work)
        .await
        .map_err(|_| Error::unavailable("the mail server did not answer in time"))?
}

/// 把邮件服务器的回答写成一句话；不带命令、不带密码。
fn refused(error: async_imap::error::Error) -> Error {
    Error::unavailable(format!("the mail server answered: {error}"))
}

async fn open(settings: &MailboxSettings, secret: &str) -> Result<Session<MailStream>> {
    let tcp = connect(settings).await?;

    let stream = match settings.security {
        MailboxSecurity::Tls => MailStream::Tls(Box::new(start_tls(&settings.host, tcp).await?)),
        MailboxSecurity::None => MailStream::Plain(tcp),
        MailboxSecurity::Starttls => {
            let mut client = Client::new(tcp);

            read_greeting(&mut client).await?;

            client
                .run_command_and_check_ok("STARTTLS", None)
                .await
                .map_err(refused)?;

            let tcp = client.into_inner();
            let stream = MailStream::Tls(Box::new(start_tls(&settings.host, tcp).await?));

            return log_in(Client::new(stream), settings, secret).await;
        }
    };

    let mut client = Client::new(stream);

    read_greeting(&mut client).await?;
    log_in(client, settings, secret).await
}

/// 服务器一连上先说一句话。
async fn read_greeting<S>(client: &mut Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + fmt::Debug + Send,
{
    client
        .read_response()
        .await
        .map_err(|error| Error::unavailable(format!("the mail server said nothing: {error}")))?
        .ok_or_else(|| Error::unavailable("the mail server closed the connection"))?;

    Ok(())
}

async fn connect(settings: &MailboxSettings) -> Result<TcpStream> {
    let host = settings.host.trim_start_matches('[').trim_end_matches(']');

    let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host, settings.port))
        .await
        .map_err(|error| Error::unavailable(format!("`{host}` could not be resolved: {error}")))?
        .collect();

    if addresses.is_empty() {
        return Err(Error::unavailable(format!(
            "`{host}` resolves to no address"
        )));
    }

    if let Some(address) = addresses.iter().find(|a| is_metadata_address(a.ip())) {
        return Err(Error::forbidden(format!(
            "`{host}` resolves to `{}`, where cloud metadata services answer",
            address.ip()
        )));
    }

    let tcp = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addresses.as_slice()))
        .await
        .map_err(|_| {
            Error::unavailable(format!("{host}:{} did not answer in time", settings.port))
        })?
        .map_err(|error| {
            Error::unavailable(format!("cannot reach {host}:{}: {error}", settings.port))
        })?;

    let _ = tcp.set_nodelay(true);
    Ok(tcp)
}

async fn start_tls(host: &str, tcp: TcpStream) -> Result<TlsStream<TcpStream>> {
    let host = host.trim_start_matches('[').trim_end_matches(']');

    let name = ServerName::try_from(host.to_string())
        .map_err(|_| Error::invalid("the mail server's name cannot be used for TLS"))?;

    let provider = CryptoProvider::get_default()
        .cloned()
        .unwrap_or_else(|| Arc::new(aws_lc_rs::default_provider()));

    let verifier = Verifier::new(provider.clone())
        .map_err(|error| Error::internal(format!("cannot load the system trust store: {error}")))?;

    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|error| Error::internal(format!("TLS client: {error}")))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();

    tokio::time::timeout(
        CONNECT_TIMEOUT,
        TlsConnector::from(Arc::new(config)).connect(name, tcp),
    )
    .await
    .map_err(|_| Error::unavailable(format!("TLS with {host} did not finish in time")))?
    .map_err(|error| Error::unavailable(format!("TLS with {host} failed: {error}")))
}

async fn log_in(
    client: Client<MailStream>,
    settings: &MailboxSettings,
    secret: &str,
) -> Result<Session<MailStream>> {
    match &settings.auth {
        MailboxAuth::Password => client
            .login(&settings.username, secret)
            .await
            .map_err(|(error, _)| refused(error)),
        MailboxAuth::Oauth2 {
            token_url,
            client_id,
            scope,
        } => {
            let token = client_credentials_token(token_url, client_id, secret, scope).await?;

            let authenticator = XOAuth2 {
                user: settings.username.clone(),
                token,
            };

            client
                .authenticate("XOAUTH2", authenticator)
                .await
                .map_err(|(error, _)| refused(error))
        }
    }
}

/// `XOAUTH2`：`user=…^Aauth=Bearer …^A^A`。服务器说令牌不对时会先发一段错误说明，再要一次回答，这时回空。
struct XOAuth2 {
    user: String,
    token: String,
}

impl Authenticator for XOAuth2 {
    type Response = String;

    fn process(&mut self, challenge: &[u8]) -> String {
        if challenge.is_empty() {
            format!(
                "user={}\u{1}auth=Bearer {}\u{1}\u{1}",
                self.user, self.token
            )
        } else {
            String::new()
        }
    }
}

/// 用客户端凭证换一个令牌。
async fn client_credentials_token(
    token_url: &str,
    client_id: &str,
    client_secret: &str,
    scope: &str,
) -> Result<String> {
    let url = url::Url::parse(token_url)
        .map_err(|error| Error::invalid(format!("the token address: {error}")))?;

    let client = pinned_client(&url, TOKEN_TIMEOUT).await?;

    let response = client
        .post(url)
        .form(&[
            ("grant_type", "client_credentials"),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("scope", scope),
        ])
        .send()
        .await
        .map_err(|error| {
            Error::unavailable(format!("the token address could not be reached: {error}"))
        })?;

    let status = response.status();
    let answer: serde_json::Value = response.json().await.unwrap_or_default();

    let Some(token) = answer["access_token"]
        .as_str()
        .filter(|_| status.is_success())
    else {
        let reason = answer["error_description"]
            .as_str()
            .or_else(|| answer["error"].as_str())
            .unwrap_or_default();

        return Err(Error::unavailable(format!(
            "the token address answered {status}: {reason}"
        )));
    };

    Ok(token.to_string())
}

async fn read(
    session: &mut Session<MailStream>,
    mailbox: &Mailbox,
    limit: usize,
    max_bytes: u32,
) -> Result<MailBatch> {
    let folder = session
        .examine(&mailbox.settings.folder)
        .await
        .map_err(refused)?;

    let uid_validity = folder
        .uid_validity
        .ok_or_else(|| Error::unavailable("the mail server does not keep stable message ids"))?;

    let Some(cursor) = mailbox
        .cursor
        .filter(|cursor| cursor.uid_validity == uid_validity)
    else {
        // 第一次连上，或邮箱重排了：从现在的位置开始，存量邮件不算。
        let last_uid = match folder.uid_next {
            Some(next) => next.saturating_sub(1),
            None => newest_uid(session).await?,
        };

        return Ok(MailBatch {
            start_at: Some(MailCursor {
                uid_validity,
                last_uid,
            }),
            uid_validity,
            mails: Vec::new(),
        });
    };

    // `n:*` 没有更新的邮件时也会答最后一封：要自己再筛一遍。
    let mut uids: Vec<u32> = session
        .uid_search(format!("UID {}:*", cursor.last_uid.saturating_add(1)))
        .await
        .map_err(refused)?
        .into_iter()
        .filter(|uid| *uid > cursor.last_uid)
        .collect();

    uids.sort_unstable();
    uids.truncate(limit);

    let mut mails = Vec::with_capacity(uids.len());

    for uid in uids {
        mails.push(fetch(session, uid, max_bytes).await?);
    }

    Ok(MailBatch {
        start_at: None,
        uid_validity,
        mails,
    })
}

/// 最新一封的 UID：服务器没给 `UIDNEXT` 时才用。
async fn newest_uid(session: &mut Session<MailStream>) -> Result<u32> {
    let uids = session.uid_search("ALL").await.map_err(refused)?;

    Ok(uids.into_iter().max().unwrap_or(0))
}

/// 取一封邮件：先问大小，太大的不取正文。
async fn fetch(session: &mut Session<MailStream>, uid: u32, max_bytes: u32) -> Result<FetchedMail> {
    let size = {
        let fetches: Vec<_> = session
            .uid_fetch(uid.to_string(), "(UID RFC822.SIZE)")
            .await
            .map_err(refused)?
            .try_collect()
            .await
            .map_err(refused)?;

        fetches
            .iter()
            .find(|fetch| fetch.uid == Some(uid))
            .and_then(|fetch| fetch.size)
    };

    if size.is_some_and(|size| size > max_bytes) {
        return Ok(FetchedMail { uid, raw: None });
    }

    let fetches: Vec<_> = session
        .uid_fetch(uid.to_string(), "(UID BODY.PEEK[])")
        .await
        .map_err(refused)?
        .try_collect()
        .await
        .map_err(refused)?;

    let raw = fetches
        .iter()
        .find(|fetch| fetch.uid == Some(uid))
        .and_then(|fetch| fetch.body())
        .map(<[u8]>::to_vec);

    Ok(FetchedMail { uid, raw })
}

#[cfg(test)]
mod tests;
