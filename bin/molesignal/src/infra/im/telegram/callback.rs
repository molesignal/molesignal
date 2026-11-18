// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What an alert card's button hands back when pressed. Telegram keeps at most 64 bytes of
//! `callback_data`, so the [`AlertButtonValue`] is written compactly:
//! `<action>:<notification>:<alert id>[:<policy id>]`.
//!
//! The action is one letter (`a`cknowledge, `r`esolve, `s`nooze, approve as `y`es, reject as
//! `n`o), and so are the alert notifications (`t`riggered, `e`scalated, ac`k`nowledged,
//! resol`v`ed) and the approval request (re`q`uested); another notification type is written
//! whole. A UUID is its 16 bytes in base64url behind a `~`; any other ID is written as it
//! is (MoleSignal's are 27-character KSUIDs). When the policy does not fit, it is left out and the
//! card is rewritten with the default template. "Acknowledge all" carries up to 20 alerts and gets
//! no button.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use domain::notify::buttons::{AlertAction, AlertButtonValue};
use uuid::Uuid;

/// Telegram's limit on a button's `callback_data`.
pub(crate) const MAX_DATA_BYTES: usize = 64;

const ACTIONS: [(AlertAction, &str); 5] = [
    (AlertAction::Acknowledge, "a"),
    (AlertAction::Resolve, "r"),
    (AlertAction::Snooze, "s"),
    (AlertAction::Approve, "y"),
    (AlertAction::Reject, "n"),
];

const NOTIFICATIONS: [(&str, &str); 5] = [
    ("alert.triggered", "t"),
    ("alert.escalated", "e"),
    ("alert.acknowledged", "k"),
    ("alert.resolved", "v"),
    ("agent.approval.requested", "q"),
];

/// The button's data; `None` when the action takes no button or the IDs do not fit.
pub(crate) fn encode(value: &AlertButtonValue) -> Option<String> {
    let action = ACTIONS
        .iter()
        .find(|(action, _)| *action == value.action)
        .map(|(_, code)| *code)?;

    let notification = match NOTIFICATIONS
        .iter()
        .find(|(name, _)| *name == value.event_type)
    {
        Some((_, code)) => (*code).to_string(),
        // Written whole: longer than a letter, so it never reads as one.
        None if value.event_type.chars().count() > 1 && !value.event_type.contains(':') => {
            value.event_type.clone()
        }
        None => return None,
    };

    let alert = pack(&value.alert_id)?;
    let short = format!("{action}:{notification}:{alert}");

    let long = value
        .policy_id
        .as_deref()
        .and_then(pack)
        .map(|policy| format!("{short}:{policy}"));

    long.filter(|data| data.len() <= MAX_DATA_BYTES)
        .or_else(|| (short.len() <= MAX_DATA_BYTES).then_some(short))
}

/// The value a pressed button carries; `None` for data that is not an alert button's.
pub(crate) fn decode(data: &str) -> Option<AlertButtonValue> {
    let parts = data.split(':').collect::<Vec<_>>();

    let (action, notification, alert, policy) = match parts.as_slice() {
        [action, notification, alert] => (*action, *notification, *alert, None),
        [action, notification, alert, policy] => (*action, *notification, *alert, Some(*policy)),
        _ => return None,
    };

    let action = ACTIONS
        .iter()
        .find(|(_, code)| *code == action)
        .map(|(action, _)| *action)?;

    let event_type = match NOTIFICATIONS.iter().find(|(_, code)| *code == notification) {
        Some((name, _)) => (*name).to_string(),
        None if notification.chars().count() > 1 => notification.to_string(),
        None => return None,
    };

    Some(AlertButtonValue {
        action,
        alert_id: unpack(alert)?,
        event_type,
        policy_id: match policy {
            Some(policy) => Some(unpack(policy)?),
            None => None,
        },
        alert_ids: Vec::new(),
    })
}

/// An ID as written in the data: a UUID packed, anything else as it is (unless it could be
/// mistaken for a packed one or a separator).
fn pack(id: &str) -> Option<String> {
    if id.is_empty() || id.contains(':') || id.starts_with('~') {
        return None;
    }

    match Uuid::parse_str(id) {
        Ok(uuid) if uuid.to_string() == id => {
            Some(format!("~{}", URL_SAFE_NO_PAD.encode(uuid.as_bytes())))
        }
        _ => Some(id.to_string()),
    }
}

fn unpack(written: &str) -> Option<String> {
    match written.strip_prefix('~') {
        Some(packed) => {
            let bytes = URL_SAFE_NO_PAD.decode(packed).ok()?;
            Uuid::from_slice(&bytes).ok().map(|uuid| uuid.to_string())
        }
        None if !written.is_empty() => Some(written.to_string()),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use kernel::ids::Id;

    use super::*;

    fn value(
        action: AlertAction,
        alert: &str,
        event: &str,
        policy: Option<&str>,
    ) -> AlertButtonValue {
        AlertButtonValue {
            action,
            alert_id: alert.into(),
            event_type: event.into(),
            policy_id: policy.map(str::to_string),
            alert_ids: Vec::new(),
        }
    }

    #[test]
    fn a_button_round_trips_within_64_bytes() {
        let ksuids = value(
            AlertAction::Acknowledge,
            Id::new().as_str(),
            "alert.triggered",
            Some(Id::new().as_str()),
        );
        let uuids = value(
            AlertAction::Snooze,
            Id::new_uuid().as_str(),
            "alert.escalated",
            Some(Id::new_uuid().as_str()),
        );
        let other = value(AlertAction::Resolve, "incident-1", "alert.reopened", None);

        for value in [ksuids, uuids, other] {
            let data = encode(&value).expect("the button fits");

            assert!(data.len() <= MAX_DATA_BYTES, "{data}");
            assert_eq!(decode(&data), Some(value));
        }
    }

    #[test]
    fn uuids_are_packed_and_the_rest_written_as_it_is() {
        let uuid = "0190f5c2-6f3a-7cc1-9d2e-8a1b2c3d4e5f";
        let data = encode(&value(
            AlertAction::Resolve,
            uuid,
            "alert.acknowledged",
            None,
        ))
        .unwrap();

        assert_eq!(data.len(), "r:k:~".len() + 22);
        assert!(data.starts_with("r:k:~"));
        assert_eq!(decode(&data).unwrap().alert_id, uuid);

        let data = encode(&value(
            AlertAction::Acknowledge,
            "inc-1",
            "alert.triggered",
            Some("p-1"),
        ));
        assert_eq!(data.as_deref(), Some("a:t:inc-1:p-1"));
    }

    #[test]
    fn a_policy_that_does_not_fit_is_left_out() {
        let alert = Id::new();
        let long = value(
            AlertAction::Acknowledge,
            alert.as_str(),
            "alert.some_new_notification",
            Some(Id::new().as_str()),
        );

        let data = encode(&long).unwrap();
        let read = decode(&data).unwrap();

        assert!(data.len() <= MAX_DATA_BYTES);
        assert_eq!(read.alert_id, alert.as_str());
        assert_eq!(read.event_type, "alert.some_new_notification");
        assert_eq!(read.policy_id, None);
    }

    #[test]
    fn an_approval_button_round_trips_with_its_policy() {
        for action in [AlertAction::Approve, AlertAction::Reject] {
            let approval = value(
                action,
                Id::new().as_str(),
                "agent.approval.requested",
                Some(Id::new().as_str()),
            );

            let data = encode(&approval).expect("the button fits");

            assert!(data.len() <= MAX_DATA_BYTES, "{data}");
            assert!(data.contains(":q:"), "{data}");
            assert_eq!(decode(&data), Some(approval));
        }
    }

    #[test]
    fn acknowledge_all_and_odd_ids_get_no_button() {
        let all = AlertButtonValue {
            alert_ids: vec!["a".into(), "b".into()],
            ..value(AlertAction::AcknowledgeAll, "a", "alert.triggered", None)
        };

        assert_eq!(encode(&all), None);
        assert_eq!(
            encode(&value(AlertAction::Resolve, "a:b", "alert.triggered", None)),
            None
        );
        assert_eq!(
            encode(&value(AlertAction::Resolve, "~x", "alert.triggered", None)),
            None
        );
        assert_eq!(
            encode(&value(
                AlertAction::Resolve,
                &"x".repeat(70),
                "alert.triggered",
                None
            )),
            None
        );
    }

    #[test]
    fn foreign_data_is_not_an_alert_button() {
        assert_eq!(decode(""), None);
        assert_eq!(decode("x:t:inc-1"), None);
        assert_eq!(decode("a:t:"), None);
        assert_eq!(decode("a:z:inc-1"), None);
        assert_eq!(decode("a:t:~!!"), None);
        assert_eq!(decode("a:t:inc-1:p-1:extra"), None);
    }
}
