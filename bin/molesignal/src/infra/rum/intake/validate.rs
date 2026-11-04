// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! One event against the contract. A bad event is refused on its own; the rest of the request is
//! kept.

use contracts::rum_event_validator;
use serde_json::{Map, Value};

use super::convert::integral;
use crate::domain::rum::event::RumEventType;

/// How far ahead of the server clock an event may claim to have begun. A device clock that is a
/// little fast is common; one that is days ahead would put the event in the wrong partition.
const MAX_FUTURE_MS: i64 = 24 * 60 * 60 * 1000;

/// An event that passed the contract, with what the server decides for itself taken out.
#[derive(Debug)]
pub struct Validated {
    pub kind: RumEventType,
    /// When the event began, in epoch milliseconds.
    pub date_ms: i64,
    /// The application the event names, when it names one. The token decides which one counts.
    pub application: Option<String>,
    pub event: Map<String, Value>,
}

pub fn validate(value: Value, now_ms: i64) -> Result<Validated, String> {
    let validator = rum_event_validator();

    if !validator.is_valid(&value) {
        let issue = validator.validate(&value).into_iter().next();

        return Err(match issue {
            Some(issue) if issue.path.is_empty() => issue.message,
            Some(issue) => format!("{}: {}", issue.path, issue.message),
            None => "event does not match the contract".into(),
        });
    }

    let Value::Object(mut event) = value else {
        return Err("event must be a JSON object".into());
    };

    let kind = event
        .get("type")
        .and_then(Value::as_str)
        .and_then(RumEventType::parse)
        .ok_or_else(|| "type is not a RUM event type".to_string())?;

    let date_ms = event.get("date").and_then(integral).unwrap_or_default();

    if date_ms > now_ms.saturating_add(MAX_FUTURE_MS) {
        return Err("date is more than a day in the future".into());
    }

    let application = event.remove("application").and_then(|application| {
        application
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
    });

    // The server alone says where a client is.
    event.remove("geo");
    event.remove("network");

    Ok(Validated {
        kind,
        date_ms,
        application,
        event,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const NOW: i64 = 1_760_000_000_000;

    fn view() -> Value {
        json!({
            "type": "view",
            "date": NOW,
            "source": "browser",
            "session": {"id": "s-1"},
            "view": {"id": "v-1", "url": "https://shop.example.com/", "revision": 1, "time_spent": 0}
        })
    }

    #[test]
    fn a_good_event_gives_its_kind() {
        let validated = validate(view(), NOW).unwrap();

        assert_eq!(validated.kind, RumEventType::View);
        assert_eq!(validated.application, None);
    }

    #[test]
    fn the_application_is_taken_out_and_reported() {
        let mut event = view();

        event["application"] = json!({"id": "shop-web"});

        let validated = validate(event, NOW).unwrap();

        assert_eq!(validated.application.as_deref(), Some("shop-web"));
        assert!(!validated.event.contains_key("application"));
    }

    #[test]
    fn what_the_server_alone_decides_is_dropped() {
        let mut event = view();

        event["geo"] = json!({"country_iso_code": "XX"});
        event["network"] = json!({"client": {"ip": "1.2.3.4"}});

        let validated = validate(event, NOW).unwrap();

        assert!(!validated.event.contains_key("geo"));
        assert!(!validated.event.contains_key("network"));
    }

    #[test]
    fn a_contract_violation_names_the_field() {
        let mut event = view();

        event["view"]["revision"] = json!(0);

        let reason = validate(event, NOW).unwrap_err();

        assert!(reason.contains("/view/revision"), "{reason}");
    }

    #[test]
    fn a_missing_field_is_named_by_the_schema() {
        let mut event = view();

        event.as_object_mut().unwrap().remove("type");

        assert!(validate(event, NOW).is_err());
        assert!(validate(json!([1, 2]), NOW).is_err());
        assert!(validate(json!("event"), NOW).is_err());
    }

    #[test]
    fn an_event_from_the_far_future_is_refused() {
        let mut event = view();

        event["date"] = json!(NOW + MAX_FUTURE_MS + 1);
        assert!(validate(event.clone(), NOW).unwrap_err().contains("future"));

        event["date"] = json!(NOW + MAX_FUTURE_MS);
        assert!(validate(event, NOW).is_ok());
    }

    #[test]
    fn unknown_fields_are_kept_for_the_extra_column() {
        let mut event = view();

        event["experiment"] = json!({"arm": "b"});

        let validated = validate(event, NOW).unwrap();

        assert_eq!(validated.event["experiment"]["arm"], "b");
    }
}
