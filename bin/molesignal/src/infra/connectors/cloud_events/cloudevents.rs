// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! CloudEvents 1.0 (<https://github.com/cloudevents/spec/blob/v1.0.2/cloudevents/spec.md>).
//!
//! - `id`, `source`, `type`, `subject` and `time` become `event_id`, `event_source`, `event_type`,
//!   `event_subject` and the event's time; `datacontenttype` and `dataschema` become
//!   `data_content_type` and `data_schema`.
//! - `data` is the payload; `data_base64` is decoded first.
//! - Any other attribute is an extension and keeps its name, such as Alibaba Cloud's
//!   `aliyunaccountid` or Tencent Cloud's `region`. Alibaba Cloud's `aliyunaccountid` and
//!   `aliyunregionid` also give `account_id` and `region`.
//! - In binary mode (the HTTP binding) the attributes come in `ce-<name>` headers, percent-encoded,
//!   the content type in `Content-Type`, and the body is the data.

use base64::Engine as _;
use serde_json::{Map, Value};

use super::{CloudEvent, Named, bytes_payload, payload, text, time_of};

/// Attributes of the specification itself: everything else is an extension.
const CONTEXT: [&str; 10] = [
    "specversion",
    "id",
    "source",
    "type",
    "subject",
    "time",
    "datacontenttype",
    "dataschema",
    "data",
    "data_base64",
];

/// An event in structured mode: the whole event is a JSON object.
pub(super) fn structured(mut object: Map<String, Value>) -> CloudEvent {
    let content_type = text(object.get("datacontenttype"));
    let data = object.remove("data");
    let encoded = object.remove("data_base64");
    let mut fields = Map::new();

    if let Some(data) = data {
        payload(&mut fields, data, content_type.as_deref());
    } else if let Some(Value::String(encoded)) = encoded {
        match base64::engine::general_purpose::STANDARD.decode(encoded.trim()) {
            Ok(bytes) => bytes_payload(&mut fields, &bytes, content_type.as_deref()),
            Err(_) => {
                fields.insert("data_base64".into(), Value::String(encoded));
            }
        }
    }

    attributes(&object, fields)
}

/// An event in binary mode: `attributes` are the `ce-*` headers without the prefix.
pub(super) fn binary(
    attributes_in_headers: &[(String, String)],
    content_type: Option<&str>,
    body: &[u8],
) -> CloudEvent {
    let mut object: Map<String, Value> = attributes_in_headers
        .iter()
        .map(|(name, value)| {
            let value = percent_encoding::percent_decode_str(value).decode_utf8_lossy();

            (name.clone(), Value::String(value.into_owned()))
        })
        .collect();

    if let Some(content_type) = content_type {
        object.insert(
            "datacontenttype".into(),
            Value::String(content_type.to_string()),
        );
    }

    let mut fields = Map::new();

    bytes_payload(&mut fields, body, content_type);
    attributes(&object, fields)
}

/// Writes the attributes beside the payload already in `fields`.
fn attributes(object: &Map<String, Value>, mut fields: Map<String, Value>) -> CloudEvent {
    for (field, attribute) in [
        ("data_content_type", "datacontenttype"),
        ("data_schema", "dataschema"),
    ] {
        if let Some(value) = text(object.get(attribute)) {
            fields.insert(field.into(), Value::String(value));
        }
    }

    for (name, value) in object {
        if !CONTEXT.contains(&name.as_str()) && !value.is_null() {
            fields.entry(name.clone()).or_insert_with(|| value.clone());
        }
    }

    let named = Named {
        id: text(object.get("id")),
        source: text(object.get("source")),
        kind: text(object.get("type")),
        subject: text(object.get("subject")),
        account: text(object.get("aliyunaccountid")),
        region: text(object.get("aliyunregionid")).or_else(|| text(object.get("region"))),
        time: time_of(object.get("time")),
    };

    named.into_event("cloudevents", fields)
}

#[cfg(test)]
mod tests {
    use domain::intake::RawEvent;
    use kernel::time::TimestampMicros;

    use super::{
        super::{Delivery, Request, read},
        *,
    };

    fn one(body: &str) -> CloudEvent {
        match read(body.as_bytes(), &Request::default()).unwrap() {
            Delivery::Events(mut events) => events.remove(0),
            Delivery::Validation(code) => panic!("validation {code}"),
        }
    }

    #[test]
    fn the_specification_example_keeps_its_extensions_and_its_text_data() {
        // The example of the CloudEvents JSON format specification.
        let event = one(r#"{
            "specversion" : "1.0",
            "type" : "com.github.pull_request.opened",
            "source" : "https://github.com/cloudevents/spec/pull",
            "subject" : "123",
            "id" : "A234-1234-1234",
            "time" : "2018-04-05T17:31:00Z",
            "comexampleextension1" : "value",
            "comexampleothervalue" : 5,
            "datacontenttype" : "text/xml",
            "data" : "<much wow=\"xml\"/>"
        }"#);

        let RawEvent { timestamp, fields } = &event.event;

        assert_eq!(
            event.dedup_key.as_deref(),
            Some("https://github.com/cloudevents/spec/pull\nA234-1234-1234")
        );
        assert_eq!(*timestamp, TimestampMicros::from_secs(1_522_949_460));
        assert_eq!(fields["event_id"], "A234-1234-1234");
        assert_eq!(
            fields["event_source"],
            "https://github.com/cloudevents/spec/pull"
        );
        assert_eq!(fields["event_type"], "com.github.pull_request.opened");
        assert_eq!(fields["event_subject"], "123");
        assert_eq!(fields["event_format"], "cloudevents");
        assert_eq!(fields["data_content_type"], "text/xml");
        assert_eq!(fields["message"], "<much wow=\"xml\"/>");
        assert_eq!(fields["comexampleextension1"], "value");
        assert_eq!(fields["comexampleothervalue"], 5);
        assert!(!fields.contains_key("specversion"));
    }

    #[test]
    fn an_alibaba_cloud_event_names_its_account_and_region() {
        // The OSS event of the Alibaba Cloud EventBridge docs.
        let event = one(r#"{
            "datacontenttype": "application/json;charset=utf-8",
            "aliyunaccountid": "1234567890981234",
            "data": {"region": "cn-hangzhou", "eventName": "ObjectReplication:ObjectCreated"},
            "subject": "acs.oss:cn-hangzhou:1234567890981234:215672:/examplebucket/text.txt",
            "aliyunoriginalaccountid": "1234567890981234",
            "source": "acs.oss",
            "type": "oss:ObjectReplication:ObjectCreated",
            "aliyunpublishtime": "2021-08-12T05:52:31.000Z",
            "specversion": "1.0",
            "aliyuneventbusname": "default",
            "id": "c9ca7be0-6f27-4a7c-a9c0-a7e8f5bf1234",
            "time": "2021-08-12T13:52:31+08:00",
            "aliyunregionid": "cn-hangzhou",
            "aliyunpublishaddr": "10.0.0.1"
        }"#);

        let fields = &event.event.fields;

        assert_eq!(fields["account_id"], "1234567890981234");
        assert_eq!(fields["region"], "cn-hangzhou");
        assert_eq!(
            fields["data"]["eventName"],
            "ObjectReplication:ObjectCreated"
        );
        assert_eq!(fields["aliyuneventbusname"], "default");
        assert_eq!(
            event.event.timestamp,
            TimestampMicros::from_secs(1_628_747_551)
        );
    }

    #[test]
    fn a_tencent_cloud_event_has_its_region_and_a_time_in_millis() {
        // The CVM event of the Tencent Cloud EventBridge docs.
        let event = one(r#"{
            "specversion": "1.0",
            "id": "13a3f42d-7258-4ada-da6d-023a333b4662",
            "source": "cvm.cloud.tencent",
            "type": "cvm:ErrorEvent:GuestReboot",
            "subject": "qcs::cvm:ap-guangzhou:uin/1250000000:instance/ins-xxxxxxx",
            "time": "1615430559146",
            "region": "ap-guangzhou",
            "datacontenttype": "application/json;charset=utf-8",
            "status": "1",
            "tags": {"key1": "value1"},
            "data": {"uin": "1250000000", "instanceId": "ins-xxxxxxx"}
        }"#);

        let fields = &event.event.fields;

        assert_eq!(fields["region"], "ap-guangzhou");
        assert_eq!(fields["status"], "1");
        assert_eq!(fields["tags"]["key1"], "value1");
        assert_eq!(fields["data"]["instanceId"], "ins-xxxxxxx");
        assert_eq!(
            event.event.timestamp,
            TimestampMicros::from_millis(1_615_430_559_146)
        );
    }

    #[test]
    fn data_base64_is_decoded() {
        let event = one(
            r#"{"specversion": "1.0", "id": "1", "source": "s", "type": "t",
                            "data_base64": "eyJrIjogInYifQ=="}"#,
        );

        assert_eq!(event.event.fields["data"]["k"], "v");
    }

    #[test]
    fn a_binary_mode_event_takes_its_attributes_from_the_headers() {
        let headers = vec![
            ("specversion".to_string(), "1.0".to_string()),
            ("id".to_string(), "42".to_string()),
            ("source".to_string(), "%2Fmycontext".to_string()),
            ("type".to_string(), "com.example.someevent".to_string()),
            ("time".to_string(), "2018-04-05T17:31:00Z".to_string()),
        ];

        let request = Request {
            binary: headers,
            content_type: Some("application/json"),
            event_grid_type: None,
        };

        let Delivery::Events(events) = read(br#"{"temperature": 21}"#, &request).unwrap() else {
            panic!("no events");
        };

        let fields = &events[0].event.fields;

        assert_eq!(events[0].dedup_key.as_deref(), Some("/mycontext\n42"));
        assert_eq!(fields["event_source"], "/mycontext");
        assert_eq!(fields["data"]["temperature"], 21);
        assert_eq!(fields["data_content_type"], "application/json");

        let text = Request {
            content_type: Some("text/plain"),
            ..request
        };

        let Delivery::Events(events) = read(b"not json at all", &text).unwrap() else {
            panic!("no events");
        };

        assert_eq!(events[0].event.fields["message"], "not json at all");
    }
}
