// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The Azure Event Grid event schema
//! (<https://learn.microsoft.com/en-us/azure/event-grid/event-schema>).
//!
//! `id`, `topic`, `eventType`, `subject` and `eventTime` become `event_id`, `event_source`,
//! `event_type`, `event_subject` and the event's time; `data` is the payload and `dataVersion`
//! is `data_version`. The subscription in the topic (`/subscriptions/<id>/…`) is `account_id`.
//!
//! A new subscription is validated with a `Microsoft.EventGrid.SubscriptionValidationEvent`
//! (header `aeg-event-type: SubscriptionValidation`), answered with its `data.validationCode`
//! (<https://learn.microsoft.com/en-us/azure/event-grid/end-point-validation-event-grid-events-schema>).

use kernel::{Error, Result};
use serde_json::{Map, Value};

use super::{CloudEvent, Named, payload, text, time_of};

/// The event type of a subscription validation.
const VALIDATION_EVENT_TYPE: &str = "Microsoft.EventGrid.SubscriptionValidationEvent";

/// Whether an object reads as an Event Grid event: an `eventType` and an `eventTime`.
pub(super) fn is_event(object: &Map<String, Value>) -> bool {
    object.get("eventType").is_some_and(Value::is_string) && object.contains_key("eventTime")
}

pub(super) fn event(mut object: Map<String, Value>) -> CloudEvent {
    let mut fields = Map::new();

    if let Some(data) = object.remove("data") {
        payload(&mut fields, data, None);
    }

    if let Some(version) = text(object.get("dataVersion")) {
        fields.insert("data_version".into(), Value::String(version));
    }

    let topic = text(object.get("topic"));

    let named = Named {
        id: text(object.get("id")),
        account: topic.as_deref().and_then(subscription_of),
        source: topic,
        kind: text(object.get("eventType")),
        subject: text(object.get("subject")),
        region: None,
        time: time_of(object.get("eventTime")),
    };

    named.into_event("event_grid", fields)
}

/// The Azure subscription of a topic: `/subscriptions/<id>/resourceGroups/…`.
fn subscription_of(topic: &str) -> Option<String> {
    let mut parts = topic.trim_start_matches('/').split('/');

    parts
        .next()
        .filter(|part| part.eq_ignore_ascii_case("subscriptions"))?;

    parts.next().filter(|id| !id.is_empty()).map(str::to_string)
}

/// The code to echo when the request validates a subscription; `None` for any other request.
pub(super) fn validation_code(items: &[Value], header: Option<&str>) -> Result<Option<String>> {
    let asked =
        header.is_some_and(|value| value.trim().eq_ignore_ascii_case("SubscriptionValidation"));

    let validation = items
        .iter()
        .find(|item| item.get("eventType").and_then(Value::as_str) == Some(VALIDATION_EVENT_TYPE));

    match validation {
        Some(event) => text(event.pointer("/data/validationCode"))
            .map(Some)
            .ok_or_else(|| Error::invalid("the Event Grid validation has no validationCode")),
        None if asked => Err(Error::invalid(
            "the request says it validates an Event Grid subscription but holds no validation \
             event",
        )),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use kernel::time::TimestampMicros;

    use super::super::{Delivery, Request, read};

    /// The validation event of the Event Grid docs.
    const VALIDATION: &str = r#"[{
        "id": "2d1781af-3a4c-4d7c-bd0c-e34b19da4e66",
        "topic": "/subscriptions/xx/resourceGroups/xx/providers/Microsoft.Storage/storageAccounts/xx",
        "subject": "",
        "data": {
            "validationCode": "512d38b6-c7b8-40c8-89fe-f46f9e9622b6",
            "validationUrl": "https://rp-eastus2.eventgrid.azure.net:553/eventsubscriptions/myeventsub/validate?id=0000000000-0000-0000-0000-00000000000000&t=2022-10-28T04:23:35.1981776Z&apiVersion=2018-05-01-preview&token=1A1A1A1A"
        },
        "eventType": "Microsoft.EventGrid.SubscriptionValidationEvent",
        "eventTime": "2022-10-28T04:23:35.1981776Z",
        "metadataVersion": "1",
        "dataVersion": "1"
    }]"#;

    #[test]
    fn a_subscription_validation_is_answered_with_its_code() {
        let request = Request {
            event_grid_type: Some("SubscriptionValidation"),
            ..Request::default()
        };

        assert!(matches!(
            read(VALIDATION.as_bytes(), &request).unwrap(),
            Delivery::Validation(code) if code == "512d38b6-c7b8-40c8-89fe-f46f9e9622b6"
        ));

        // The event type alone is enough, and the header alone is not.
        assert!(matches!(
            read(VALIDATION.as_bytes(), &Request::default()).unwrap(),
            Delivery::Validation(_)
        ));
        assert!(read(br#"[{"eventType": "Other", "eventTime": "x"}]"#, &request).is_err());
        assert!(
            read(
                br#"[{"eventType": "Microsoft.EventGrid.SubscriptionValidationEvent", "data": {}}]"#,
                &Request::default()
            )
            .is_err()
        );
    }

    #[test]
    fn a_blob_created_event_keeps_its_data_and_names_its_subscription() {
        // The Blob Storage example of the Event Grid event schema.
        let body = br#"[{
            "topic": "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/Storage/providers/Microsoft.Storage/storageAccounts/xstoretestaccount",
            "subject": "/blobServices/default/containers/testcontainer/blobs/testfile.txt",
            "eventType": "Microsoft.Storage.BlobCreated",
            "eventTime": "2017-06-26T18:41:00.9584103Z",
            "id": "831e1650-001e-001b-66ab-eeb76e069631",
            "data": {"api": "PutBlockList", "contentType": "text/plain", "contentLength": 524288},
            "dataVersion": "",
            "metadataVersion": "1"
        }]"#;

        let Delivery::Events(events) = read(body, &Request::default()).unwrap() else {
            panic!("no events");
        };

        let event = &events[0];
        let fields = &event.event.fields;

        assert_eq!(fields["event_type"], "Microsoft.Storage.BlobCreated");
        assert_eq!(
            fields["event_subject"],
            "/blobServices/default/containers/testcontainer/blobs/testfile.txt"
        );
        assert_eq!(fields["account_id"], "00000000-0000-0000-0000-000000000000");
        assert_eq!(fields["data"]["api"], "PutBlockList");
        assert_eq!(fields["event_format"], "event_grid");
        assert!(!fields.contains_key("data_version"));
        assert_eq!(
            event.event.timestamp,
            TimestampMicros(1_498_502_460_958_410)
        );
        assert!(
            event
                .dedup_key
                .as_deref()
                .unwrap()
                .ends_with("\n831e1650-001e-001b-66ab-eeb76e069631")
        );
    }
}
