// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 邮件来源读的 IMAP 邮箱：配置，以及告警节点上定时读（[`MailboxPoller`]）。
//!
//! 邮件读到以后走和 HTTP 接入一样的处理（[`AlertSources::process`]），收到的每一封都留在「最近收到」里。
//! 读到哪了记在邮箱上，一封处理完才往前走，节点中途挂了最多重读一封，同一个去重键不会重复开事件。
//! 来源关着时不读，也不留着位置：重新打开时从那时开始，关着的时候来的邮件不补。

use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use tokio::{task::JoinHandle, time::MissedTickBehavior};

use super::{AlertSources, IntakeReply, intake::blank_record};
use crate::{
    app::cluster::WorkOwnership,
    domain::alerting::source::{
        AlertSource, AlertSourceKind, IntakeStatus, MailCursor, Mailbox, MailboxRepository,
        MailboxSettings,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 归属与重叠指标里这项工作的名字。
const WORK: &str = "alert_mailboxes";
/// 多久看一次哪些邮箱该读了；各邮箱多久读一次写在它自己的设置上。
const ROUND_SECS: u64 = 10;
/// 一个邮箱一次最多处理几封：一下来很多时分几次，不占着别的邮箱。
const MAILS_PER_READ: usize = 50;
/// 一封邮件最大多少字节，和 HTTP 接入的上限一样。
pub const MAX_MAIL_BYTES: u32 = 4 * 1024 * 1024;
/// 同时读几个邮箱。
const CONCURRENT_READS: usize = 8;

/// 读到的一封邮件；太大的 `raw` 为空。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedMail {
    pub uid: u32,
    pub raw: Option<Vec<u8>>,
}

/// 一次读到的东西。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailBatch {
    /// 第一次连上或邮箱重排了：从这里开始，这次没有要处理的邮件。
    pub start_at: Option<MailCursor>,
    pub uid_validity: u32,
    /// UID 从小到大。
    pub mails: Vec<FetchedMail>,
}

/// 试连邮箱的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MailboxStatus {
    pub messages: u32,
}

/// 读邮箱的那一层。
#[async_trait]
pub trait MailboxClient: Send + Sync {
    /// 读 `mailbox.cursor` 之后的新邮件，最多 `limit` 封，大于 `max_bytes` 的不取正文。
    async fn read_new(&self, mailbox: &Mailbox, limit: usize, max_bytes: u32) -> Result<MailBatch>;
    /// 连得上、登录得了、文件夹在。
    async fn check(&self, settings: &MailboxSettings, secret: &str) -> Result<MailboxStatus>;
}

/// 读到的邮件交给谁处理。
#[async_trait]
trait MailIntake: Send + Sync {
    async fn mail(&self, source: &AlertSource, raw: &[u8]) -> IntakeReply;
    async fn too_large(&self, source: &AlertSource, uid: u32);
}

#[async_trait]
impl MailIntake for AlertSources {
    async fn mail(&self, source: &AlertSource, raw: &[u8]) -> IntakeReply {
        self.process(source, "", Some("message/rfc822"), raw).await
    }

    async fn too_large(&self, source: &AlertSource, uid: u32) {
        let now = TimestampMicros::now();
        let mut record = blank_record(source, "", IntakeStatus::Rejected, 413, now);

        record.message = Some(format!(
            "mail {uid} is larger than {} MiB",
            MAX_MAIL_BYTES / (1024 * 1024)
        ));

        self.record(record).await;
    }
}

/// 改邮箱时写的部分。
#[derive(Debug, Clone)]
pub struct MailboxWrite {
    pub settings: MailboxSettings,
    /// 新的密码或客户端密钥；`None` 保持原样。
    pub secret: Option<String>,
}

impl AlertSources {
    /// 一个来源的邮箱（带着秘密）；没配时为空。
    pub async fn mailbox(&self, org_id: &Id, source_id: &Id) -> Result<Option<Mailbox>> {
        let source = self.get(org_id, source_id).await?;
        self.mailboxes.get(&source.id).await
    }

    /// 配邮箱：要在来源的接收集群上配，发件人名单要写。换了地方（主机、账号、文件夹）时重新从当时开始。
    pub async fn save_mailbox(
        &self,
        org_id: &Id,
        source_id: &Id,
        mut write: MailboxWrite,
    ) -> Result<Mailbox> {
        let source = self.get(org_id, source_id).await?;
        self.check_mail_source(&source).await?;

        if !source.email.names_senders() {
            return Err(Error::invalid(
                "name the senders this source accepts first: anyone who knows the address could \
                 otherwise open incidents (`*` accepts everyone)",
            ));
        }

        write.settings.validate()?;

        let existing = self.mailboxes.get(&source.id).await?;

        let secret = write
            .secret
            .filter(|secret| !secret.is_empty())
            .or_else(|| existing.as_ref().map(|mailbox| mailbox.secret.clone()))
            .ok_or_else(|| Error::invalid("the mailbox needs a password or client secret"))?;

        let keeps_place = existing
            .as_ref()
            .is_some_and(|mailbox| mailbox.settings.same_place(&write.settings));

        let mailbox = Mailbox {
            source_id: source.id.clone(),
            org_id: source.org_id.clone(),
            settings: write.settings,
            secret,
            cursor: existing
                .as_ref()
                .filter(|_| keeps_place)
                .and_then(|mailbox| mailbox.cursor),
            last_polled_at: None,
            last_success_at: existing
                .as_ref()
                .and_then(|mailbox| mailbox.last_success_at),
            last_error: String::new(),
            updated_at: TimestampMicros::now(),
        };

        self.mailboxes.save(mailbox).await
    }

    pub async fn delete_mailbox(&self, org_id: &Id, source_id: &Id) -> Result<()> {
        let source = self.get(org_id, source_id).await?;
        self.mailboxes.delete(&source.id).await
    }

    /// 试连：设置来自表单，秘密没填时用存着的。
    pub async fn check_mailbox(
        &self,
        org_id: &Id,
        source_id: &Id,
        mut write: MailboxWrite,
    ) -> Result<MailboxStatus> {
        let source = self.get(org_id, source_id).await?;
        self.check_mail_source(&source).await?;
        write.settings.validate()?;

        let stored = match write.secret.filter(|secret| !secret.is_empty()) {
            Some(secret) => Some(secret),
            None => self
                .mailboxes
                .get(&source.id)
                .await?
                .map(|mailbox| mailbox.secret),
        };

        let secret = stored
            .ok_or_else(|| Error::invalid("the mailbox needs a password or client secret"))?;

        let client = self
            .mail_client
            .as_ref()
            .ok_or_else(|| Error::unavailable("this node does not read mailboxes"))?;

        client.check(&write.settings, &secret).await
    }

    /// 邮箱只配给本集群开事件的邮件来源。
    async fn check_mail_source(&self, source: &AlertSource) -> Result<()> {
        if source.kind != AlertSourceKind::Email {
            return Err(Error::invalid("only an email source reads a mailbox"));
        }

        let local = self.local_cluster().await?;

        if !source.opens_incidents_on(&local) {
            return Err(Error::invalid(format!(
                "the mailbox is read on the receiving cluster of this source ({}): set it up there",
                source.home_cluster
            )));
        }

        Ok(())
    }
}

/// 告警节点上定时读邮箱。多个节点时每个邮箱归一个节点读。
pub struct MailboxPoller {
    sources: Arc<AlertSources>,
    mailboxes: Arc<dyn MailboxRepository>,
    client: Arc<dyn MailboxClient>,
    ownership: Arc<WorkOwnership>,
}

impl MailboxPoller {
    pub fn new(
        sources: Arc<AlertSources>,
        mailboxes: Arc<dyn MailboxRepository>,
        client: Arc<dyn MailboxClient>,
        ownership: Arc<WorkOwnership>,
    ) -> Self {
        Self {
            sources,
            mailboxes,
            client,
            ownership,
        }
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(ROUND_SECS));
            interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

            loop {
                interval.tick().await;

                if let Err(error) = self.round(TimestampMicros::now()).await {
                    tracing::warn!(%error, "mailbox round failed");
                }
            }
        })
    }

    async fn round(&self, now: TimestampMicros) -> Result<()> {
        let all = self.mailboxes.list_all().await?;
        let view = self.ownership.view().await;
        let mut owned = 0;
        let mut due = Vec::new();

        for mailbox in all {
            if view.owns(mailbox.source_id.as_str()) {
                owned += 1;

                if mailbox.due(now) {
                    due.push(mailbox);
                }
            }
        }

        self.ownership.record_owned(WORK, owned);

        if due.is_empty() {
            return Ok(());
        }

        let local = self.sources.local_cluster().await?;

        futures::stream::iter(due)
            .for_each_concurrent(CONCURRENT_READS, |mailbox| {
                let local = local.as_str();

                async move {
                    if let Err(error) = self.read(mailbox, local, now).await {
                        tracing::warn!(%error, "a mailbox could not be read");
                    }
                }
            })
            .await;

        Ok(())
    }

    async fn read(&self, mut mailbox: Mailbox, local: &str, now: TimestampMicros) -> Result<()> {
        let source = match self.sources.sources.get(&mailbox.source_id).await {
            Ok(source) => source,
            Err(Error::NotFound(_)) => return Ok(()),
            Err(error) => return Err(error),
        };

        if !source.opens_incidents_on(local) {
            return Ok(());
        }

        if !source.enabled {
            // 关着时不留位置：重新打开后从那时开始。
            if mailbox.cursor.take().is_some() {
                self.mailboxes.save(mailbox).await?;
            }

            return Ok(());
        }

        let outcome = read_mailbox(
            self.client.as_ref(),
            self.mailboxes.as_ref(),
            self.sources.as_ref(),
            &source,
            &mailbox,
        )
        .await;

        let error = outcome.err().map(|error| error.to_string());

        if let Some(error) = &error {
            tracing::warn!(source_id = %source.id, %error, "a mailbox could not be read");
        }

        let error: Option<String> = error.map(|error| error.chars().take(500).collect());

        self.mailboxes
            .record_poll(&mailbox.source_id, now, error.as_deref())
            .await
    }
}

/// 读一个邮箱：记下位置，或一封一封处理、一封一封往前走。处理不了（数据库不通）时停下，下次接着读。
async fn read_mailbox(
    client: &dyn MailboxClient,
    mailboxes: &dyn MailboxRepository,
    intake: &dyn MailIntake,
    source: &AlertSource,
    mailbox: &Mailbox,
) -> Result<()> {
    let batch = client
        .read_new(mailbox, MAILS_PER_READ, MAX_MAIL_BYTES)
        .await?;

    if let Some(cursor) = batch.start_at {
        return mailboxes.advance(&mailbox.source_id, cursor).await;
    }

    for mail in batch.mails {
        match &mail.raw {
            Some(raw) => {
                let reply = intake.mail(source, raw).await;

                if reply.status >= 500 {
                    return Err(Error::unavailable(format!(
                        "mail {} could not be handled: {}",
                        mail.uid, reply.body["message"]
                    )));
                }
            }
            None => intake.too_large(source, mail.uid).await,
        }

        mailboxes
            .advance(
                &mailbox.source_id,
                MailCursor {
                    uid_validity: batch.uid_validity,
                    last_uid: mail.uid,
                },
            )
            .await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use serde_json::json;

    use super::*;
    use crate::domain::alerting::{
        incident::Severity,
        source::{EmailRules, MailboxAuth, MailboxSecurity},
    };

    fn settings() -> MailboxSettings {
        MailboxSettings {
            host: "imap.acme.com".into(),
            port: 993,
            security: MailboxSecurity::Tls,
            username: "alerts@acme.com".into(),
            auth: MailboxAuth::Password,
            folder: "INBOX".into(),
            poll_secs: 60,
        }
    }

    fn mailbox() -> Mailbox {
        Mailbox {
            source_id: Id::from_string("source"),
            org_id: Id::from_string("org"),
            settings: settings(),
            secret: "p".into(),
            cursor: Some(MailCursor {
                uid_validity: 7,
                last_uid: 5,
            }),
            last_polled_at: None,
            last_success_at: None,
            last_error: String::new(),
            updated_at: TimestampMicros(1),
        }
    }

    fn source() -> AlertSource {
        AlertSource {
            id: Id::from_string("source"),
            org_id: Id::from_string("org"),
            name: "mail".into(),
            kind: AlertSourceKind::Email,
            enabled: true,
            escalation_policy_id: None,
            severity: Severity::Warning,
            labels: Default::default(),
            token_prefix: "x".into(),
            token_hash: String::new(),
            home_cluster: String::new(),
            created_by: None,
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
            last_intake_at: None,
            email: EmailRules::default(),
        }
    }

    struct Reader(MailBatch);

    #[async_trait]
    impl MailboxClient for Reader {
        async fn read_new(&self, _: &Mailbox, _: usize, _: u32) -> Result<MailBatch> {
            Ok(self.0.clone())
        }

        async fn check(&self, _: &MailboxSettings, _: &str) -> Result<MailboxStatus> {
            Ok(MailboxStatus { messages: 0 })
        }
    }

    #[derive(Default)]
    struct Places(Mutex<Vec<MailCursor>>);

    #[async_trait]
    impl MailboxRepository for Places {
        async fn get(&self, _: &Id) -> Result<Option<Mailbox>> {
            Ok(None)
        }

        async fn list_all(&self) -> Result<Vec<Mailbox>> {
            Ok(Vec::new())
        }

        async fn save(&self, mailbox: Mailbox) -> Result<Mailbox> {
            Ok(mailbox)
        }

        async fn delete(&self, _: &Id) -> Result<()> {
            Ok(())
        }

        async fn advance(&self, _: &Id, cursor: MailCursor) -> Result<()> {
            self.0.lock().unwrap().push(cursor);
            Ok(())
        }

        async fn record_poll(&self, _: &Id, _: TimestampMicros, _: Option<&str>) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct Intake {
        taken: Mutex<Vec<Vec<u8>>>,
        too_large: Mutex<Vec<u32>>,
        fail_on: Option<Vec<u8>>,
    }

    #[async_trait]
    impl MailIntake for Intake {
        async fn mail(&self, _: &AlertSource, raw: &[u8]) -> IntakeReply {
            if self.fail_on.as_deref() == Some(raw) {
                return IntakeReply {
                    status: 503,
                    body: json!({"message": "retry"}),
                };
            }

            self.taken.lock().unwrap().push(raw.to_vec());

            IntakeReply {
                status: 200,
                body: json!({}),
            }
        }

        async fn too_large(&self, _: &AlertSource, uid: u32) {
            self.too_large.lock().unwrap().push(uid);
        }
    }

    fn batch(mails: Vec<FetchedMail>) -> MailBatch {
        MailBatch {
            start_at: None,
            uid_validity: 7,
            mails,
        }
    }

    fn mail(uid: u32, raw: &[u8]) -> FetchedMail {
        FetchedMail {
            uid,
            raw: Some(raw.to_vec()),
        }
    }

    #[tokio::test]
    async fn the_first_read_only_remembers_where_it_starts() {
        let start = MailCursor {
            uid_validity: 7,
            last_uid: 40,
        };

        let reader = Reader(MailBatch {
            start_at: Some(start),
            uid_validity: 7,
            mails: Vec::new(),
        });

        let places = Places::default();
        let intake = Intake::default();

        read_mailbox(&reader, &places, &intake, &source(), &mailbox())
            .await
            .unwrap();

        assert_eq!(*places.0.lock().unwrap(), vec![start]);
        assert!(intake.taken.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn each_mail_moves_the_place_forward_even_when_it_is_refused_or_too_large() {
        let reader = Reader(batch(vec![
            mail(6, b"one"),
            FetchedMail { uid: 7, raw: None },
            mail(8, b"three"),
        ]));

        let places = Places::default();
        let intake = Intake::default();

        read_mailbox(&reader, &places, &intake, &source(), &mailbox())
            .await
            .unwrap();

        assert_eq!(
            places
                .0
                .lock()
                .unwrap()
                .iter()
                .map(|c| c.last_uid)
                .collect::<Vec<_>>(),
            vec![6, 7, 8]
        );
        assert_eq!(intake.taken.lock().unwrap().len(), 2);
        assert_eq!(*intake.too_large.lock().unwrap(), vec![7]);
    }

    #[tokio::test]
    async fn a_mail_that_could_not_be_handled_is_read_again_next_time() {
        let reader = Reader(batch(vec![
            mail(6, b"one"),
            mail(7, b"two"),
            mail(8, b"three"),
        ]));
        let places = Places::default();

        let intake = Intake {
            fail_on: Some(b"two".to_vec()),
            ..Intake::default()
        };

        let error = read_mailbox(&reader, &places, &intake, &source(), &mailbox())
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("mail 7"), "{error}");
        assert_eq!(
            places
                .0
                .lock()
                .unwrap()
                .iter()
                .map(|c| c.last_uid)
                .collect::<Vec<_>>(),
            vec![6]
        );
    }
}
