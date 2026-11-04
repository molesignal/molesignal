// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
};

use super::*;
use crate::{
    domain::alerting::source::{MailCursor, MailboxSettings},
    shared::{ids::Id, time::TimestampMicros},
};

const MAIL: &str = "From: nagios@acme.com\r\nSubject: PROBLEM disk\r\n\r\nDisk full\r\n";

/// 本机的假邮件服务器：说 IMAP4rev1 里这里用到的那几句，记下收到的命令。
struct FakeServer {
    address: SocketAddr,
    commands: Arc<Mutex<Vec<String>>>,
}

#[derive(Clone)]
struct Folder {
    uid_validity: u32,
    uid_next: u32,
    /// (uid, 大小, 内容)
    mails: Vec<(u32, u32, String)>,
    password: String,
}

impl FakeServer {
    async fn start(folder: Folder) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let commands = Arc::new(Mutex::new(Vec::new()));
        let seen = commands.clone();

        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let (folder, seen) = (folder.clone(), seen.clone());
                tokio::spawn(async move { serve(stream, folder, seen).await });
            }
        });

        Self { address, commands }
    }

    fn settings(&self) -> MailboxSettings {
        MailboxSettings {
            host: self.address.ip().to_string(),
            port: self.address.port(),
            security: MailboxSecurity::None,
            username: "alerts@acme.com".into(),
            auth: MailboxAuth::Password,
            folder: "INBOX".into(),
            poll_secs: 60,
        }
    }

    fn mailbox(&self, cursor: Option<MailCursor>) -> Mailbox {
        Mailbox {
            source_id: Id::from_string("source"),
            org_id: Id::from_string("org"),
            settings: self.settings(),
            secret: "right".into(),
            cursor,
            last_polled_at: None,
            last_success_at: None,
            last_error: String::new(),
            updated_at: TimestampMicros(1),
        }
    }

    fn commands(&self) -> Vec<String> {
        self.commands.lock().unwrap().clone()
    }
}

async fn serve(stream: TcpStream, folder: Folder, seen: Arc<Mutex<Vec<String>>>) {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read);

    write.write_all(b"* OK IMAP4rev1 ready\r\n").await.unwrap();

    loop {
        let mut line = String::new();

        if lines.read_line(&mut line).await.unwrap_or(0) == 0 {
            return;
        }

        let line = line.trim_end().to_string();
        seen.lock().unwrap().push(line.clone());

        let (tag, command) = line.split_once(' ').unwrap();
        let upper = command.to_ascii_uppercase();

        let reply = if upper.starts_with("LOGIN") {
            if command.contains(&format!("\"{}\"", folder.password)) {
                format!("{tag} OK LOGIN completed\r\n")
            } else {
                format!("{tag} NO [AUTHENTICATIONFAILED] Invalid credentials\r\n")
            }
        } else if upper.starts_with("EXAMINE") {
            format!(
                "* {} EXISTS\r\n* 0 RECENT\r\n* FLAGS (\\Seen)\r\n* OK [UIDVALIDITY {}] ok\r\n\
                 * OK [UIDNEXT {}] ok\r\n{tag} OK [READ-ONLY] EXAMINE completed\r\n",
                folder.mails.len(),
                folder.uid_validity,
                folder.uid_next,
            )
        } else if upper.starts_with("UID SEARCH") {
            let from: u32 = upper
                .split("UID ")
                .nth(2)
                .and_then(|range| range.split(':').next())
                .and_then(|from| from.parse().ok())
                .unwrap_or(0);

            let mut uids: Vec<String> = folder
                .mails
                .iter()
                .filter(|(uid, _, _)| *uid >= from)
                .map(|(uid, _, _)| uid.to_string())
                .collect();

            // 协议的怪处：`n:*` 没有更新的邮件时也会答最后一封。
            if uids.is_empty()
                && let Some((uid, _, _)) = folder.mails.last()
            {
                uids.push(uid.to_string());
            }

            format!(
                "* SEARCH {}\r\n{tag} OK SEARCH completed\r\n",
                uids.join(" ")
            )
        } else if upper.starts_with("UID FETCH") {
            let uid: u32 = upper
                .split_whitespace()
                .nth(2)
                .and_then(|uid| uid.parse().ok())
                .unwrap_or(0);

            let found = folder.mails.iter().find(|(known, _, _)| *known == uid);

            match found {
                Some((uid, size, _)) if upper.contains("RFC822.SIZE") => format!(
                    "* 1 FETCH (UID {uid} RFC822.SIZE {size})\r\n{tag} OK FETCH completed\r\n"
                ),
                Some((uid, _, body)) => format!(
                    "* 1 FETCH (UID {uid} BODY[] {{{}}}\r\n{body})\r\n{tag} OK FETCH completed\r\n",
                    body.len()
                ),
                None => format!("{tag} OK FETCH completed\r\n"),
            }
        } else if upper.starts_with("LOGOUT") {
            write
                .write_all(format!("* BYE\r\n{tag} OK LOGOUT completed\r\n").as_bytes())
                .await
                .unwrap();
            return;
        } else {
            format!("{tag} BAD unknown command\r\n")
        };

        write.write_all(reply.as_bytes()).await.unwrap();
    }
}

fn folder(mails: Vec<(u32, u32, String)>) -> Folder {
    Folder {
        uid_validity: 7,
        uid_next: mails.last().map_or(1, |(uid, _, _)| uid + 1),
        mails,
        password: "right".into(),
    }
}

#[tokio::test]
async fn the_first_read_starts_from_now_and_leaves_the_backlog_alone() {
    let server = FakeServer::start(folder(vec![(5, 10, MAIL.into()), (6, 10, MAIL.into())])).await;

    let batch = ImapMailboxClient
        .read_new(&server.mailbox(None), 50, 1024)
        .await
        .unwrap();

    assert!(batch.mails.is_empty());
    assert_eq!(
        batch.start_at,
        Some(MailCursor {
            uid_validity: 7,
            last_uid: 6
        })
    );

    // 只读：`EXAMINE`，不是 `SELECT`。
    let commands = server.commands();
    assert!(
        commands
            .iter()
            .any(|command| command.contains("EXAMINE \"INBOX\"")
                || command.contains("EXAMINE INBOX")),
        "{commands:?}"
    );
    assert!(!commands.iter().any(|command| command.contains("SELECT")));
}

#[tokio::test]
async fn new_mail_is_read_in_order_without_marking_it_seen() {
    let server = FakeServer::start(folder(vec![
        (5, 10, MAIL.into()),
        (
            6,
            10,
            "From: a@b.com\r\nSubject: second\r\n\r\nx\r\n".into(),
        ),
        (9, 10, "From: a@b.com\r\nSubject: third\r\n\r\ny\r\n".into()),
    ]))
    .await;

    let cursor = MailCursor {
        uid_validity: 7,
        last_uid: 5,
    };

    let batch = ImapMailboxClient
        .read_new(&server.mailbox(Some(cursor)), 50, 1024)
        .await
        .unwrap();

    assert_eq!(batch.start_at, None);
    assert_eq!(
        batch.mails.iter().map(|mail| mail.uid).collect::<Vec<_>>(),
        vec![6, 9]
    );
    assert!(
        String::from_utf8_lossy(batch.mails[0].raw.as_ref().unwrap()).contains("Subject: second")
    );

    let commands = server.commands();
    assert!(
        commands
            .iter()
            .any(|command| command.contains("BODY.PEEK[]"))
    );
    assert!(!commands.iter().any(|command| command.contains("STORE")));
}

#[tokio::test]
async fn a_read_with_nothing_new_gets_nothing_though_the_server_answers_the_last_mail() {
    let server = FakeServer::start(folder(vec![(5, 10, MAIL.into())])).await;

    let cursor = MailCursor {
        uid_validity: 7,
        last_uid: 5,
    };

    let batch = ImapMailboxClient
        .read_new(&server.mailbox(Some(cursor)), 50, 1024)
        .await
        .unwrap();

    assert!(batch.mails.is_empty());
    assert_eq!(batch.start_at, None);
}

#[tokio::test]
async fn a_mailbox_that_was_rebuilt_starts_over_from_now() {
    let server = FakeServer::start(folder(vec![(3, 10, MAIL.into())])).await;

    let cursor = MailCursor {
        uid_validity: 1,
        last_uid: 900,
    };

    let batch = ImapMailboxClient
        .read_new(&server.mailbox(Some(cursor)), 50, 1024)
        .await
        .unwrap();

    assert!(batch.mails.is_empty());
    assert_eq!(
        batch.start_at,
        Some(MailCursor {
            uid_validity: 7,
            last_uid: 3
        })
    );
}

#[tokio::test]
async fn a_read_takes_at_most_the_limit_and_skips_the_body_of_a_huge_mail() {
    let server = FakeServer::start(folder(vec![
        (6, 9_000, MAIL.into()),
        (7, 10, MAIL.into()),
        (8, 10, MAIL.into()),
    ]))
    .await;

    let cursor = MailCursor {
        uid_validity: 7,
        last_uid: 5,
    };

    let batch = ImapMailboxClient
        .read_new(&server.mailbox(Some(cursor)), 2, 1024)
        .await
        .unwrap();

    assert_eq!(batch.mails.len(), 2);
    assert_eq!(batch.mails[0].uid, 6);
    assert!(batch.mails[0].raw.is_none());
    assert_eq!(batch.mails[1].uid, 7);
    assert!(batch.mails[1].raw.is_some());
}

#[tokio::test]
async fn a_wrong_password_says_what_the_server_said() {
    let server = FakeServer::start(folder(Vec::new())).await;
    let mut mailbox = server.mailbox(None);
    mailbox.secret = "wrong".into();

    let error = ImapMailboxClient
        .read_new(&mailbox, 50, 1024)
        .await
        .unwrap_err()
        .to_string();

    assert!(error.contains("Invalid credentials"), "{error}");
    assert!(!error.contains("wrong"), "{error}");
}

#[tokio::test]
async fn checking_a_mailbox_counts_its_messages() {
    let server = FakeServer::start(folder(vec![(5, 10, MAIL.into())])).await;

    let status = ImapMailboxClient
        .check(&server.settings(), "right")
        .await
        .unwrap();

    assert_eq!(status.messages, 1);
}

#[tokio::test]
async fn a_host_that_cannot_be_reached_says_so() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let settings = MailboxSettings {
        host: "127.0.0.1".into(),
        port,
        security: MailboxSecurity::None,
        username: "u".into(),
        auth: MailboxAuth::Password,
        folder: "INBOX".into(),
        poll_secs: 60,
    };

    assert!(ImapMailboxClient.check(&settings, "p").await.is_err());
}

#[tokio::test]
async fn cloud_metadata_addresses_are_refused() {
    let settings = MailboxSettings {
        host: "169.254.169.254".into(),
        port: 993,
        security: MailboxSecurity::Tls,
        username: "u".into(),
        auth: MailboxAuth::Password,
        folder: "INBOX".into(),
        poll_secs: 60,
    };

    let error = ImapMailboxClient.check(&settings, "p").await.unwrap_err();
    assert!(matches!(error, Error::Forbidden(_)), "{error}");
}

#[test]
fn xoauth2_answers_with_the_user_and_token_then_gives_up_on_an_error() {
    let mut auth = XOAuth2 {
        user: "alerts@acme.com".into(),
        token: "tok".into(),
    };

    assert_eq!(
        auth.process(b""),
        "user=alerts@acme.com\u{1}auth=Bearer tok\u{1}\u{1}"
    );
    assert_eq!(auth.process(b"{\"status\":\"401\"}"), "");
}
