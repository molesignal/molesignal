// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Presses on the alert buttons of `mattermost_bot` posts:
//! `POST /api/v1/public/notify/{connector_id}/mattermost?token=…`.
//!
//! Every button names this address with the connector's callback token ([`callback_url`]), and the
//! Mattermost server posts each press here: who pressed (user ID and username), where, and the
//! button's context, an [`AlertButtonValue`]. Mattermost keeps the address from its clients, so the
//! token shows the press comes from a post this connector made. A Mattermost bot is not one of Mole
//! Agent's bots: the presser is known by the verified notification methods on this connector, as on
//! WeCom app cards ([`super::wecom_app`]). The answer replaces the post with the alert as it is now
//! and tells only the presser what went wrong.
//!
//! A wrong token is 401; a connector that is gone, disabled or not a Mattermost bot is 404.

use std::time::Duration;

use agent::im::{CardAction, CardAnswer, ConversationKind, NoticeKind, Sender};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, State},
    routing::post,
};
use domain::notify::{
    buttons::{AlertButtonValue, AlertButtons},
    connector::NotifyConnector,
};
use kernel::{Error, Result, ids::Id};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    alerts::carry_out,
    inbound::{Identity, identify_on},
    texts::Texts,
};
use crate::{
    api::AppState,
    app::notify::ReplyCallbacks,
    infra::notify::adapters::{MATTERMOST_BOT_CONNECTOR_TYPE, mattermost_post},
};

const MAX_BODY_BYTES: usize = 64 * 1024;
/// How long a press waits for its answer: Mattermost waits 30 seconds by default.
const ANSWER_WITHIN: Duration = Duration::from_secs(20);
/// The language pack's name for the platform, when it has one.
const PLATFORM_KEY: &str = "platform.mattermost";

pub(crate) fn public_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/public/notify/{connectorId}/mattermost",
            post(pressed),
        )
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
}

/// The address a connector's buttons post their presses to; none without an external URL.
pub(super) fn callback_url(external_url: &str, connector_id: &Id, token: &str) -> Option<String> {
    let base = external_url.trim().trim_end_matches('/');

    (!base.is_empty()).then(|| {
        format!(
            "{base}/api/v1/public/notify/{}/mattermost?token={token}",
            connector_id.as_str()
        )
    })
}

#[derive(Debug, Default, Deserialize)]
struct PressQuery {
    #[serde(default)]
    token: Option<String>,
}

async fn pressed(
    State(state): State<AppState>,
    Path(connector_id): Path<String>,
    Query(query): Query<PressQuery>,
    body: Bytes,
) -> Result<Json<Value>> {
    let callbacks = state.alerting.reply_callbacks.clone();
    let connector = callback_connector(&callbacks, &connector_id, query.token.as_deref()).await?;

    let request: Value = serde_json::from_slice(&body)
        .map_err(|_| Error::invalid("the body is not a Mattermost action request"))?;

    // Not one of the alert buttons: nothing changes.
    let Some(action) = card_action(&request) else {
        return Ok(Json(json!({})));
    };

    let token = callbacks.token(&connector.id);
    let callback = callback_url(&state.platform.external_url, &connector.id, &token);

    // In a task of its own, the press is carried out even when Mattermost stops waiting.
    let settled = tokio::spawn(async move { answer(&state, &connector, &action).await });

    match tokio::time::timeout(ANSWER_WITHIN, settled).await {
        Ok(Ok(answer)) => Ok(Json(response(&answer, callback.as_deref()))),
        Ok(Err(error)) => {
            tracing::warn!(%error, "a Mattermost alert button press stopped");
            Ok(Json(json!({})))
        }
        Err(_) => {
            tracing::warn!("a Mattermost alert button press outlasted its answer");
            Ok(Json(json!({})))
        }
    }
}

/// The connector a press is for, when the press brings that connector's callback token. A wrong
/// token is refused before anything is looked up, so it tells nothing about which connectors exist.
async fn callback_connector(
    callbacks: &ReplyCallbacks,
    connector_id: &str,
    token: Option<&str>,
) -> Result<NotifyConnector> {
    let id = Id::from_string(connector_id);

    if !callbacks.token_matches(&id, token.unwrap_or_default()) {
        return Err(Error::unauthorized("invalid callback token"));
    }

    callbacks
        .connector(&id)
        .await?
        .filter(|connector| {
            connector.enabled && connector.connector_type == MATTERMOST_BOT_CONNECTOR_TYPE
        })
        .ok_or_else(|| Error::not_found("no such callback"))
}

/// The press in Mattermost's request: the button's value, and who pressed it by user ID, username,
/// and username with `@` as people write it on their notification method.
fn card_action(request: &Value) -> Option<CardAction> {
    let value = serde_json::from_value::<AlertButtonValue>(request.get("context")?.clone()).ok()?;

    let user_id = request["user_id"]
        .as_str()
        .map(str::trim)
        .filter(|id| !id.is_empty())?;

    let user_name = request["user_name"]
        .as_str()
        .map(str::trim)
        .filter(|name| !name.is_empty());

    let mut ids = vec![user_id.to_string()];

    if let Some(name) = user_name {
        ids.push(name.to_string());
        ids.push(format!("@{name}"));
    }

    Some(CardAction {
        value,
        sender: Sender {
            ids,
            name: user_name.map(str::to_string),
        },
        conversation_id: request["channel_id"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        // Mattermost does not say whether the channel is direct.
        kind: ConversationKind::Group,
        reply: json!({}),
        card: json!({ "post_id": request["post_id"] }),
    })
}

/// Carries out a press; when that fails, the presser is told why.
async fn answer(state: &AppState, connector: &NotifyConnector, action: &CardAction) -> CardAnswer {
    match settle(state, connector, action).await {
        Ok(answer) => answer,
        Err(error) => {
            tracing::warn!(connector = %connector.id, %error, "a Mattermost alert button press failed");

            let texts = Texts::for_user(state, &connector.organization_id, None).await;
            told(
                NoticeKind::Error,
                texts.get("alert.failed", &[("reason", &error.to_string())]),
            )
        }
    }
}

async fn settle(
    state: &AppState,
    connector: &NotifyConnector,
    action: &CardAction,
) -> Result<CardAnswer> {
    let org_id = &connector.organization_id;
    let identity = identify_on(state, org_id, &connector.id, &action.sender).await?;

    if let Identity::User(user_id) = &identity {
        return carry_out(state, org_id, user_id, action).await;
    }

    let texts = Texts::for_user(state, org_id, None).await;
    Ok(unknown(&texts, &identity))
}

/// The answer to a presser the press cannot be put down to: linked to nobody, or to several users.
fn unknown(texts: &Texts, identity: &Identity) -> CardAnswer {
    let key = match identity {
        Identity::Ambiguous => "alert.ambiguous",
        Identity::User(_) | Identity::Unknown => "alert.unlinked",
    };

    told(
        NoticeKind::Warning,
        texts.get(key, &[("platform", &platform(texts))]),
    )
}

/// The platform's name in a sentence: "Mattermost", unless the language pack names it otherwise.
fn platform(texts: &Texts) -> String {
    let named = texts.get(PLATFORM_KEY, &[]);

    if named == format!("agent.im.{PLATFORM_KEY}") {
        "Mattermost".to_string()
    } else {
        named
    }
}

fn told(kind: NoticeKind, text: String) -> CardAnswer {
    CardAnswer {
        notice: Some((kind, text)),
        card: None,
    }
}

/// Mattermost's answer to the press: the post rewritten as the alert is now, with buttons that
/// still post here, and a line only the presser sees when the press did not simply go through.
fn response(answer: &CardAnswer, callback: Option<&str>) -> Value {
    let mut response = json!({});

    if let Some(card) = &answer.card {
        let buttons = AlertButtons {
            callback: callback.map(str::to_string),
            ..card.buttons.clone()
        };

        let post = mattermost_post(&card.message, Some(&buttons), card.note.as_deref());
        response["update"] = json!({ "message": post.message, "props": post.props() });
    }

    if let Some((kind, text)) = &answer.notice
        && (answer.card.is_none() || *kind != NoticeKind::Success)
    {
        response["ephemeral_text"] = json!(text);
    }

    response
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Arc};

    use agent::im::AlertCard;
    use async_trait::async_trait;
    use domain::notify::{
        buttons::AlertAction,
        connector::{ConnectorStatus, ConnectorTestStatus, NotifyMessage},
        reply::{NotifyReplyRepository, ReplyRef},
        repositories::NotifyConnectorRepository,
    };
    use kernel::time::TimestampMicros;

    use super::*;

    const CALLBACK: &str = "https://ms.example.com/api/v1/public/notify/conn-1/mattermost?token=t";

    struct NoReplies;

    #[async_trait]
    impl NotifyReplyRepository for NoReplies {
        async fn record(&self, reply: ReplyRef) -> Result<ReplyRef> {
            Ok(reply)
        }

        async fn get(&self, _: &str, _: TimestampMicros) -> Result<Option<ReplyRef>> {
            Ok(None)
        }

        async fn find(
            &self,
            _: &Id,
            _: &Id,
            _: &str,
            _: Option<u8>,
            _: TimestampMicros,
        ) -> Result<Option<ReplyRef>> {
            Ok(None)
        }

        async fn next_slot(&self, _: &Id, _: &Id, _: &str) -> Result<u8> {
            Ok(1)
        }
    }

    /// The connectors a callback can find, by ID.
    struct Connectors(Vec<NotifyConnector>);

    #[async_trait]
    impl NotifyConnectorRepository for Connectors {
        async fn create(&self, connector: NotifyConnector) -> Result<NotifyConnector> {
            Ok(connector)
        }

        async fn update(&self, connector: NotifyConnector) -> Result<NotifyConnector> {
            Ok(connector)
        }

        async fn get(&self, _: &Id, _: &Id) -> Result<NotifyConnector> {
            Err(Error::not_found("connector"))
        }

        async fn find(&self, id: &Id) -> Result<Option<NotifyConnector>> {
            Ok(self.0.iter().find(|connector| connector.id == *id).cloned())
        }

        async fn list(&self, _: &Id) -> Result<Vec<NotifyConnector>> {
            Ok(self.0.clone())
        }

        async fn record_test_result(
            &self,
            _: &Id,
            _: &Id,
            _: TimestampMicros,
            _: ConnectorTestStatus,
            _: Option<String>,
        ) -> Result<NotifyConnector> {
            Err(Error::not_found("connector"))
        }

        async fn delete(&self, _: &Id, _: &Id) -> Result<()> {
            Ok(())
        }
    }

    fn connector(id: &str, connector_type: &str, enabled: bool) -> NotifyConnector {
        NotifyConnector {
            id: Id::from_string(id),
            organization_id: Id::from_string("org-1"),
            name: id.into(),
            connector_type: connector_type.into(),
            config: json!({}),
            capabilities: Default::default(),
            enabled,
            status: ConnectorStatus::Connected,
            last_tested_at: None,
            last_test_status: None,
            last_test_error: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn callbacks() -> ReplyCallbacks {
        ReplyCallbacks::new(
            [7; 32],
            Arc::new(NoReplies),
            Arc::new(Connectors(vec![
                connector("conn-1", MATTERMOST_BOT_CONNECTOR_TYPE, true),
                connector("conn-off", MATTERMOST_BOT_CONNECTOR_TYPE, false),
                connector("conn-slack", "slack_app", true),
            ])),
        )
    }

    fn alert_card(actions: Vec<(AlertAction, String)>, note: Option<&str>) -> AlertCard {
        AlertCard {
            message: NotifyMessage {
                title: "CRITICAL · Checkout errors".into(),
                text: "Alert Checkout errors is acknowledged.".into(),
                markdown: None,
                html: None,
                metadata: BTreeMap::from([("severity".to_string(), "critical".to_string())]),
                provider_templates: Default::default(),
                event: None,
                card: None,
            },
            buttons: AlertButtons {
                alert_id: "inc-1".into(),
                event_type: "alert.triggered".into(),
                actions,
                open: Some((
                    "Open in MoleSignal".into(),
                    "https://ms.example.com/alerts/incidents/inc-1".into(),
                )),
                ..AlertButtons::default()
            },
            note: note.map(str::to_string),
        }
    }

    #[tokio::test]
    async fn a_press_needs_the_connectors_own_token() {
        let callbacks = callbacks();
        let token = callbacks.token(&Id::from_string("conn-1"));

        let found = callback_connector(&callbacks, "conn-1", Some(&token))
            .await
            .unwrap();
        assert_eq!(found.id.as_str(), "conn-1");

        for presented in [None, Some(""), Some("wrong")] {
            let refused = callback_connector(&callbacks, "conn-1", presented).await;
            assert!(
                matches!(refused, Err(Error::Unauthorized(_))),
                "{presented:?}"
            );
        }

        // Another connector's token opens nothing here.
        let other = callbacks.token(&Id::from_string("conn-slack"));
        assert!(matches!(
            callback_connector(&callbacks, "conn-1", Some(&other)).await,
            Err(Error::Unauthorized(_))
        ));

        // Gone, disabled or not a Mattermost bot: as if there were no such callback.
        for id in ["conn-gone", "conn-off", "conn-slack"] {
            let token = callbacks.token(&Id::from_string(id));
            assert!(
                matches!(
                    callback_connector(&callbacks, id, Some(&token)).await,
                    Err(Error::NotFound(_))
                ),
                "{id}"
            );
        }
    }

    #[test]
    fn the_callback_lives_under_the_external_url() {
        let id = Id::from_string("conn-1");

        assert_eq!(
            callback_url(" https://ms.example.com/ ", &id, "t").as_deref(),
            Some(CALLBACK)
        );
        assert_eq!(callback_url("  ", &id, "t"), None);
    }

    #[test]
    fn a_press_names_the_presser_by_id_and_username() {
        let request = json!({
            "user_id": "rd49ehbqyjytddasoownkuqrxe",
            "user_name": "alice",
            "channel_id": "j6j53p28k6urx15fpcgsr20psq",
            "post_id": "gqrnh3675jfxzftnjyjfe4udeh",
            "team_id": "",
            "context": {
                "action": "resolve",
                "alert_id": "inc-1",
                "event_type": "alert.escalated",
                "policy_id": "policy-1",
            },
        });

        let action = card_action(&request).unwrap();

        assert_eq!(action.value.action, AlertAction::Resolve);
        assert_eq!(action.value.alert_id, "inc-1");
        assert_eq!(action.value.policy_id.as_deref(), Some("policy-1"));
        assert_eq!(
            action.sender.ids,
            vec!["rd49ehbqyjytddasoownkuqrxe", "alice", "@alice"]
        );
        assert_eq!(action.sender.name.as_deref(), Some("alice"));
        assert_eq!(action.conversation_id, "j6j53p28k6urx15fpcgsr20psq");
        assert_eq!(action.card["post_id"], "gqrnh3675jfxzftnjyjfe4udeh");

        // Not an alert button, or nobody to put it down to.
        assert!(card_action(&json!({ "user_id": "u1", "context": { "x": 1 } })).is_none());
        assert!(card_action(&json!({ "context": request["context"] })).is_none());
    }

    #[test]
    fn a_done_press_rewrites_the_post_with_buttons_that_still_post_here() {
        let answer = CardAnswer {
            notice: Some((NoticeKind::Success, "Acknowledged.".into())),
            card: Some(alert_card(
                vec![(AlertAction::Resolve, "Resolve".into())],
                Some("Alice acknowledged this alert."),
            )),
        };

        let answered = response(&answer, Some(CALLBACK));
        let update = &answered["update"];
        let attachment = &update["props"]["attachments"][0];
        let button = &attachment["actions"][0];

        assert_eq!(update["message"], "");
        assert_eq!(attachment["title"], "CRITICAL · Checkout errors");
        assert_eq!(attachment["footer"], "Alice acknowledged this alert.");
        assert_eq!(button["id"], "resolve");
        assert_eq!(button["integration"]["url"], CALLBACK);
        assert_eq!(
            button["integration"]["context"],
            json!({ "action": "resolve", "alert_id": "inc-1", "event_type": "alert.triggered" })
        );

        // The rewritten card says who did it; only a press that went wrong says more.
        assert!(answered.get("ephemeral_text").is_none());

        let already = CardAnswer {
            notice: Some((NoticeKind::Info, "Already resolved.".into())),
            card: Some(alert_card(Vec::new(), Some("Bob resolved this alert."))),
        };
        let answered = response(&already, Some(CALLBACK));
        assert_eq!(answered["ephemeral_text"], "Already resolved.");
        assert!(
            answered["update"]["props"]["attachments"][0]
                .get("actions")
                .is_none()
        );
    }

    #[test]
    fn a_press_without_a_card_only_answers_the_presser() {
        let answer = told(NoticeKind::Warning, "No access.".into());

        assert_eq!(
            response(&answer, Some(CALLBACK)),
            json!({ "ephemeral_text": "No access." })
        );
        assert_eq!(
            response(
                &CardAnswer {
                    notice: None,
                    card: None,
                },
                None
            ),
            json!({})
        );
    }

    #[test]
    fn an_unknown_presser_is_asked_to_link_their_mattermost_account() {
        let texts = Texts::empty();

        assert_eq!(platform(&texts), "Mattermost");

        let unlinked = unknown(&texts, &Identity::Unknown);
        assert_eq!(
            unlinked.notice,
            Some((NoticeKind::Warning, "agent.im.alert.unlinked".to_string()))
        );
        assert!(unlinked.card.is_none());

        let ambiguous = unknown(&texts, &Identity::Ambiguous);
        assert_eq!(
            ambiguous.notice,
            Some((NoticeKind::Warning, "agent.im.alert.ambiguous".to_string()))
        );
    }
}
