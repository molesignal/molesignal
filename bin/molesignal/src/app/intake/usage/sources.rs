// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{collections::HashMap, sync::Arc};

use serde::Serialize;

use crate::{
    domain::{
        iam::{
            UserRepository,
            api_token::{ApiTokenRepository, TOKEN_MARKER},
        },
        intake::usage::source::{
            CredentialKind, RUM_REPLAY_SOURCE, UsageDimension, parse_credential_key,
            parse_stream_key, stream_source_key,
        },
        stream::{StreamRepository, StreamType},
    },
    infra::connectors::ConnectorRepository,
    shared::ids::Id,
};

/// What a source is called, as a person reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceLabel {
    /// `api_token`, `connector`, `user`, `internal`, `stream` or `service`.
    pub kind: &'static str,
    /// The kind of token or connector, or the type of the stream.
    pub subkind: Option<String>,
    pub name: String,
    /// What tells it apart from others of that name: a token's prefix, a person's email.
    pub detail: Option<String>,
    /// It no longer exists; its name is then its id.
    pub missing: bool,
}

impl SourceLabel {
    fn named(kind: &'static str, name: impl Into<String>) -> Self {
        Self {
            kind,
            subkind: None,
            name: name.into(),
            detail: None,
            missing: false,
        }
    }
}

/// The sources of a dimension: the labels of those asked about, and which ones exist whether they
/// have sent anything or not.
#[derive(Debug, Default)]
pub struct SourceBook {
    pub labels: HashMap<String, SourceLabel>,
    pub existing: Vec<String>,
}

/// Where the names of the sources come from.
pub struct IntakeSourceDirectory {
    tokens: Arc<dyn ApiTokenRepository>,
    connectors: Arc<dyn ConnectorRepository>,
    users: Arc<dyn UserRepository>,
    streams: Arc<dyn StreamRepository>,
}

impl IntakeSourceDirectory {
    pub fn new(
        tokens: Arc<dyn ApiTokenRepository>,
        connectors: Arc<dyn ConnectorRepository>,
        users: Arc<dyn UserRepository>,
        streams: Arc<dyn StreamRepository>,
    ) -> Self {
        Self {
            tokens,
            connectors,
            users,
            streams,
        }
    }

    /// Looks up the sources of `keys` and those that exist. A list that cannot be read leaves the
    /// sources in it known by their ids, not reported as gone.
    pub async fn book(&self, org_id: &Id, dimension: UsageDimension, keys: &[&str]) -> SourceBook {
        let mut book = match dimension {
            UsageDimension::Credential => self.credentials(org_id, keys).await,
            UsageDimension::Stream => self.stream_sources(org_id, keys).await,
            UsageDimension::Service => SourceBook::default(),
        };

        for key in keys {
            if !book.labels.contains_key(*key) {
                book.labels
                    .insert((*key).to_string(), unlisted(dimension, key));
            }
        }

        book
    }

    async fn credentials(&self, org_id: &Id, keys: &[&str]) -> SourceBook {
        let mut book = SourceBook::default();

        match self.tokens.list_by_org(org_id).await {
            Ok(tokens) => {
                for token in tokens {
                    let key = format!("{}:{}", CredentialKind::ApiToken.as_str(), token.id);

                    let name = if token.name.is_empty() {
                        token.prefix.clone()
                    } else {
                        token.name
                    };

                    if !token.revoked {
                        book.existing.push(key.clone());
                    }

                    book.labels.insert(
                        key,
                        SourceLabel {
                            kind: CredentialKind::ApiToken.as_str(),
                            subkind: Some(token.token_kind.as_str().to_string()),
                            name,
                            detail: Some(format!("{TOKEN_MARKER}{}", token.prefix)),
                            missing: false,
                        },
                    );
                }
            }
            Err(error) => {
                tracing::warn!(org_id = %org_id, %error, "intake usage could not read the tokens");
                known_by_id(&mut book, keys, CredentialKind::ApiToken);
            }
        }

        match self.connectors.list(org_id).await {
            Ok(connectors) => {
                for connector in connectors {
                    let key = format!("{}:{}", CredentialKind::Connector.as_str(), connector.id);

                    book.existing.push(key.clone());

                    book.labels.insert(
                        key,
                        SourceLabel {
                            kind: CredentialKind::Connector.as_str(),
                            subkind: Some(connector.kind),
                            name: connector.name,
                            detail: None,
                            missing: false,
                        },
                    );
                }
            }
            Err(error) => {
                tracing::warn!(org_id = %org_id, %error, "intake usage could not read the connectors");
                known_by_id(&mut book, keys, CredentialKind::Connector);
            }
        }

        let user_ids = keys
            .iter()
            .filter_map(|key| parse_credential_key(key).ok())
            .filter(|(kind, _)| *kind == CredentialKind::User)
            .map(|(_, id)| Id::from_string(id))
            .collect::<Vec<_>>();

        if !user_ids.is_empty() {
            match self.users.get_many(&user_ids).await {
                Ok(users) => {
                    for user in users {
                        let name = if user.display_name.is_empty() {
                            user.email.clone()
                        } else {
                            user.display_name
                        };

                        book.labels.insert(
                            format!("{}:{}", CredentialKind::User.as_str(), user.id),
                            SourceLabel {
                                detail: Some(user.email),
                                ..SourceLabel::named(CredentialKind::User.as_str(), name)
                            },
                        );
                    }
                }
                Err(error) => {
                    tracing::warn!(org_id = %org_id, %error, "intake usage could not read the users");
                    known_by_id(&mut book, keys, CredentialKind::User);
                }
            }
        }

        book
    }

    async fn stream_sources(&self, org_id: &Id, keys: &[&str]) -> SourceBook {
        let mut book = SourceBook::default();

        let streams = match self.streams.list(org_id).await {
            Ok(streams) => streams,
            Err(error) => {
                tracing::warn!(org_id = %org_id, %error, "intake usage could not read the streams");

                for key in keys {
                    if let Ok((stream_type, stream)) = parse_stream_key(key) {
                        book.labels
                            .insert((*key).to_string(), stream_label(stream_type, stream, false));
                    }
                }

                return book;
            }
        };

        for stream in streams {
            if stream.stream_type == StreamType::EXTEND {
                continue;
            }

            let key = stream_source_key(stream.stream_type, &stream.name);

            book.existing.push(key.clone());
            book.labels
                .insert(key, stream_label(stream.stream_type, &stream.name, false));
        }

        book
    }
}

fn stream_label(stream_type: StreamType, stream: &str, missing: bool) -> SourceLabel {
    SourceLabel {
        subkind: Some(stream_type.external_name().to_string()),
        missing,
        ..SourceLabel::named("stream", stream)
    }
}

/// Keys of `kind` whose list could not be read keep their ids as their names.
fn known_by_id(book: &mut SourceBook, keys: &[&str], kind: CredentialKind) {
    for key in keys {
        if let Ok((found, id)) = parse_credential_key(key)
            && found == kind
        {
            book.labels
                .insert((*key).to_string(), SourceLabel::named(kind.as_str(), id));
        }
    }
}

/// The label of a source that no list holds.
fn unlisted(dimension: UsageDimension, key: &str) -> SourceLabel {
    match dimension {
        UsageDimension::Service => SourceLabel::named("service", key),
        UsageDimension::Stream => match parse_stream_key(key) {
            Ok((stream_type, stream)) => {
                stream_label(stream_type, stream, stream != RUM_REPLAY_SOURCE)
            }
            Err(_) => SourceLabel {
                missing: true,
                ..SourceLabel::named("stream", key)
            },
        },
        UsageDimension::Credential => match parse_credential_key(key) {
            Ok((CredentialKind::Internal, _)) => {
                SourceLabel::named(CredentialKind::Internal.as_str(), "")
            }
            Ok((kind, id)) => SourceLabel {
                missing: true,
                ..SourceLabel::named(kind.as_str(), id)
            },
            Err(_) => SourceLabel {
                missing: true,
                ..SourceLabel::named(CredentialKind::ApiToken.as_str(), key)
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_no_list_holds_is_gone_unless_it_never_was_on_one() {
        let gone = unlisted(UsageDimension::Credential, "api_token:t-1");

        assert!(gone.missing);
        assert_eq!(gone.name, "t-1");
        assert_eq!(gone.kind, "api_token");

        let internal = unlisted(UsageDimension::Credential, "internal:");

        assert!(!internal.missing);
        assert_eq!(internal.kind, "internal");

        let stream = unlisted(UsageDimension::Stream, "logs:deleted");

        assert!(stream.missing);
        assert_eq!(stream.subkind.as_deref(), Some("logs"));

        let replay = unlisted(
            UsageDimension::Stream,
            &stream_source_key(StreamType::LOGS, RUM_REPLAY_SOURCE),
        );

        assert!(!replay.missing);

        let service = unlisted(UsageDimension::Service, "checkout");

        assert!(!service.missing);
        assert_eq!(service.name, "checkout");
    }
}
