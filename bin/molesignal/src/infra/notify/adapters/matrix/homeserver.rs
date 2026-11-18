// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The homeserver's client-server API (`/_matrix/client/v3`), as far as the bot uses it: who it
//! is, room aliases, joining, posting, and direct rooms through `m.direct`.

use std::{sync::LazyLock, time::Duration};

use kernel::{Error, Result};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use reqwest::{Method, header::RETRY_AFTER};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use signals::http_trace::{self, HttpTarget};
use tokio::sync::Mutex;
use url::Url;

use super::MATRIX_BOT_CONNECTOR_TYPE;
use crate::infra::{
    connectors::dispatcher::http::pinned_client,
    notify::adapters::common::{TOKENS, TokenCache, provider_message},
};

/// Path segments escape everything but letters, digits and `-_.~`: IDs and aliases hold `!`, `#`,
/// `@` and `:`.
const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// Who the bot is, kept this long per homeserver and token.
const WHOAMI_SECS: u64 = 86_400;

/// Direct rooms are created one at a time, so that notifications arriving together for someone
/// without a direct room share the room the first of them creates.
static CREATING: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

/// What the homeserver said when it refused a request.
#[derive(Debug, Default, Deserialize)]
struct Refusal {
    #[serde(skip)]
    status: u16,
    #[serde(default)]
    errcode: String,
    #[serde(default)]
    error: String,
    #[serde(default)]
    retry_after_ms: Option<u64>,
}

impl Refusal {
    fn rate_limited(&self) -> bool {
        self.status == 429 || self.errcode == "M_LIMIT_EXCEEDED"
    }

    /// `M_CODE what the homeserver said`.
    fn reason(&self) -> String {
        provider_message(&format!("{} {}", self.errcode, self.error))
    }

    /// The refusal as an error saying what could not be done; a rate limit says when to retry.
    fn into_error(self, doing: &str) -> Error {
        if self.rate_limited() {
            let hint = self
                .retry_after_ms
                .map(|ms| format!(": retry after {} s", ms.div_ceil(1_000).max(1)))
                .unwrap_or_default();

            return Error::unavailable(format!(
                "{MATRIX_BOT_CONNECTOR_TYPE} was rate limited while trying to {doing}{hint}"
            ));
        }

        Error::unavailable(format!(
            "{MATRIX_BOT_CONNECTOR_TYPE} could not {doing}: HTTP {} {}",
            self.status,
            self.reason()
        ))
    }
}

/// What the homeserver answered: the JSON of a success, or why it refused.
enum Answer {
    Done(Value),
    Refused(Refusal),
}

impl Answer {
    fn done(self, doing: &str) -> Result<Value> {
        match self {
            Self::Done(value) => Ok(value),
            Self::Refused(refusal) => Err(refusal.into_error(doing)),
        }
    }
}

/// The bot's session with its homeserver.
pub(super) struct Homeserver {
    http: reqwest::Client,
    /// The root of the API: `<homeserver>/_matrix/client/v3`.
    api: String,
    token: String,
    /// Where [`Homeserver::whoami`] keeps its answer.
    whoami_key: String,
}

impl Homeserver {
    pub(super) async fn connect(base: &Url, token: &str, timeout: Duration) -> Result<Self> {
        let http = pinned_client(base, timeout).await?;
        let root = base.as_str().trim_end_matches('/');

        Ok(Self {
            http,
            api: format!("{root}/_matrix/client/v3"),
            token: token.to_string(),
            whoami_key: TokenCache::key(MATRIX_BOT_CONNECTOR_TYPE, root, token),
        })
    }

    async fn call(&self, method: Method, path: &str, body: Option<&Value>) -> Result<Answer> {
        let mut request = self
            .http
            .request(method, format!("{}{path}", self.api))
            .bearer_auth(&self.token);

        if let Some(body) = body {
            request = request.json(body);
        }

        let response = http_trace::send(&self.http, request, HttpTarget::ThirdParty)
            .await
            .map_err(|error| {
                Error::unavailable(format!(
                    "{MATRIX_BOT_CONNECTOR_TYPE} request failed: {}",
                    error.without_url()
                ))
            })?;

        let status = response.status();

        let retry_after = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<u64>().ok());

        let text = response.text().await.unwrap_or_default();

        if status.is_success() {
            return Ok(Answer::Done(
                serde_json::from_str(&text).unwrap_or(Value::Null),
            ));
        }

        let mut refusal: Refusal = serde_json::from_str(&text).unwrap_or_default();
        refusal.status = status.as_u16();

        refusal.retry_after_ms = refusal
            .retry_after_ms
            .or(retry_after.map(|secs| secs.saturating_mul(1_000)));

        if refusal.errcode.is_empty() && refusal.error.is_empty() {
            refusal.error = text;
        }

        Ok(Answer::Refused(refusal))
    }

    /// The bot's user ID, asked once a day per homeserver and token.
    pub(super) async fn whoami(&self) -> Result<String> {
        TOKENS
            .get(&self.whoami_key, async {
                let me = self
                    .call(Method::GET, "/account/whoami", None)
                    .await?
                    .done("find out who the bot is")?;

                let user = me["user_id"]
                    .as_str()
                    .filter(|user| !user.is_empty())
                    .ok_or_else(|| {
                        Error::unavailable(format!(
                            "{MATRIX_BOT_CONNECTOR_TYPE} could not find out who the bot is: the answer named no user"
                        ))
                    })?;

                Ok((user.to_string(), WHOAMI_SECS))
            })
            .await
    }

    /// The room an alias names.
    pub(super) async fn resolve(&self, alias: &str) -> Result<String> {
        let path = format!("/directory/room/{}", segment(alias));
        let doing = format!("look up {alias}");

        match self.call(Method::GET, &path, None).await? {
            Answer::Done(room) => named(&room, "room_id", &doing),
            Answer::Refused(refusal) if refusal.errcode == "M_NOT_FOUND" => Err(Error::invalid(
                format!("{MATRIX_BOT_CONNECTOR_TYPE} room alias {alias} does not exist"),
            )),
            Answer::Refused(refusal) => Err(refusal.into_error(&doing)),
        }
    }

    /// Posts `content` into `room` and gives the event's ID. When the bot is not in the room it
    /// joins through `entry` (the alias the room was named by, or its ID) and posts again.
    pub(super) async fn post(
        &self,
        room: &str,
        entry: &str,
        txn: &str,
        content: &Value,
    ) -> Result<String> {
        let path = format!(
            "/rooms/{}/send/m.room.message/{}",
            segment(room),
            segment(txn)
        );
        let doing = format!("post in {entry}");

        match self.call(Method::PUT, &path, Some(content)).await? {
            Answer::Done(sent) => return named(&sent, "event_id", &doing),
            Answer::Refused(refusal) if refusal.errcode == "M_FORBIDDEN" => {}
            Answer::Refused(refusal) => return Err(refusal.into_error(&doing)),
        }

        self.join(entry).await?;

        let sent = self
            .call(Method::PUT, &path, Some(content))
            .await?
            .done(&doing)?;

        named(&sent, "event_id", &doing)
    }

    /// Joins a room, or says to invite the bot when the room does not let it in.
    async fn join(&self, entry: &str) -> Result<()> {
        let mut path = format!("/join/{}", segment(entry));

        // A room ID with a server part says where the room can be joined from.
        if let Some((_, server)) = entry.strip_prefix('!').and_then(|id| id.split_once(':')) {
            let server = segment(server);
            path.push_str(&format!("?via={server}&server_name={server}"));
        }

        match self.call(Method::POST, &path, Some(&json!({}))).await? {
            Answer::Done(_) => Ok(()),
            Answer::Refused(refusal) if refusal.rate_limited() => {
                Err(refusal.into_error(&format!("join {entry}")))
            }
            Answer::Refused(refusal) => {
                let bot = self.whoami().await.unwrap_or_else(|_| "the bot".into());

                Err(Error::invalid(format!(
                    "{MATRIX_BOT_CONNECTOR_TYPE} is not in {entry} and may not join it ({}); invite {bot} to the room",
                    refusal.reason()
                )))
            }
        }
    }

    /// The direct room with `person`: the newest one the bot's `m.direct` lists for them that they
    /// are still in or invited to, or else a new one, recorded there.
    pub(super) async fn direct_room(&self, person: &str) -> Result<String> {
        let me = self.whoami().await?;

        if let (_, Some(room)) = self.find_direct(&me, person).await? {
            return Ok(room);
        }

        let _creating = CREATING.lock().await;

        // Another notification may have created it while this one waited.
        let (mut direct, found) = self.find_direct(&me, person).await?;

        if let Some(room) = found {
            return Ok(room);
        }

        let room = self.create_direct(person).await?;
        let rooms = direct.entry(person).or_insert_with(|| json!([]));

        match rooms.as_array_mut() {
            Some(rooms) => rooms.push(json!(room)),
            None => *rooms = json!([room]),
        }

        // The room exists either way; unrecorded, the next notification creates another.
        let path = format!("/user/{}/account_data/m.direct", segment(&me));

        let recorded = self
            .call(Method::PUT, &path, Some(&Value::Object(direct)))
            .await
            .and_then(|answer| answer.done("record a direct room"));

        if let Err(error) = recorded {
            tracing::warn!(%error, "a new Matrix direct room is missing from m.direct");
        }

        Ok(room)
    }

    /// The bot's `m.direct`, and the newest room in it for `person` that they are in or invited to.
    async fn find_direct(
        &self,
        me: &str,
        person: &str,
    ) -> Result<(Map<String, Value>, Option<String>)> {
        let path = format!("/user/{}/account_data/m.direct", segment(me));

        let direct = match self.call(Method::GET, &path, None).await? {
            Answer::Done(Value::Object(direct)) => direct,
            Answer::Done(_) => Map::new(),
            Answer::Refused(refusal) if refusal.errcode == "M_NOT_FOUND" => Map::new(),
            Answer::Refused(refusal) => {
                return Err(refusal.into_error("read the bot's direct rooms"));
            }
        };

        let rooms = direct
            .get(person)
            .and_then(Value::as_array)
            .map(|rooms| {
                rooms
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        for room in rooms.into_iter().rev() {
            if self.present(&room, person).await? {
                return Ok((direct, Some(room)));
            }
        }

        Ok((direct, None))
    }

    /// Whether `person` is in `room` or invited to it, as far as the bot sees: not when the bot
    /// has left the room itself.
    async fn present(&self, room: &str, person: &str) -> Result<bool> {
        let path = format!(
            "/rooms/{}/state/m.room.member/{}",
            segment(room),
            segment(person)
        );

        match self.call(Method::GET, &path, None).await? {
            Answer::Done(member) => Ok(matches!(
                member["membership"].as_str(),
                Some("join" | "invite")
            )),
            Answer::Refused(refusal) if refusal.rate_limited() => {
                Err(refusal.into_error("check a direct room"))
            }
            Answer::Refused(_) => Ok(false),
        }
    }

    /// Creates a direct room with `person`, who is invited and gets the same power as the bot.
    async fn create_direct(&self, person: &str) -> Result<String> {
        let request = json!({
            "is_direct": true,
            "invite": [person],
            "preset": "trusted_private_chat",
        });

        let doing = format!("start a direct room with {person}");

        let created = self
            .call(Method::POST, "/createRoom", Some(&request))
            .await?
            .done(&doing)?;

        named(&created, "room_id", &doing)
    }
}

/// The ID an answer gives under `key`.
fn named(answer: &Value, key: &str, doing: &str) -> Result<String> {
    answer[key]
        .as_str()
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            Error::unavailable(format!(
                "{MATRIX_BOT_CONNECTOR_TYPE} could not {doing}: the answer has no {key}"
            ))
        })
}

fn segment(value: &str) -> String {
    utf8_percent_encode(value, SEGMENT).to_string()
}
