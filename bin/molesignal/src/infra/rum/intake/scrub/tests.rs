// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::{Map, Value, json};

use super::*;

fn scrubbed(text: &str) -> String {
    text::scrub(&ScrubKind::ALL, text).unwrap_or_else(|| text.to_string())
}

fn event(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(fields) => fields,
        other => panic!("not an object: {other}"),
    }
}

#[test]
fn secrets_of_every_shape_are_taken_out_of_text() {
    let leaks = [
        (
            "token eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.dBjftJeZ4CVPmB92K27uhbUJU1p1r_wW1gFWFOEjXk was refused",
            "token [secret] was refused",
        ),
        (
            "Authorization: Bearer abcdefghijklmnop1234 failed",
            "Authorization: [secret] [secret] failed",
        ),
        ("key AKIAIOSFODNN7EXAMPLE leaked", "key [secret] leaked"),
        (
            "ghp_16C7e42F292c6912E7710c838347Ae178B4a was pushed",
            "[secret] was pushed",
        ),
        (
            concat!("sk_live_", "000000000000000000000000", " ok"),
            "[secret] ok",
        ),
        ("password=hunter2&user=mei", "password=[secret]&user=mei"),
        (
            r#"{"api_key":"abc123def","name":"mei"}"#,
            r#"{"api_key":"[secret]","name":"mei"}"#,
        ),
        ("client_secret: s3cr3t-value", "client_secret: [secret]"),
        (
            "-----BEGIN RSA PRIVATE KEY-----\nMIIBOgIBAAJB\n-----END RSA PRIVATE KEY----- after",
            "[secret] after",
        ),
    ];

    for (leak, clean) in leaks {
        assert_eq!(scrubbed(leak), clean, "{leak}");
    }
}

#[test]
fn text_that_only_talks_about_secrets_is_left_alone() {
    for text in [
        "Failed to fetch /api/orders: 500 Internal Server Error",
        "Cannot read properties of undefined (reading 'price')",
        "the password must contain 8 characters",
        "Unexpected token '<' at position 0",
        "Bearer scheme is not supported",
    ] {
        assert_eq!(text::scrub(&ScrubKind::ALL, text), None, "{text}");
    }
}

#[test]
fn emails_are_taken_out_and_what_only_looks_like_one_is_not() {
    assert_eq!(
        scrubbed("sent to jane.doe+tag@example.co.uk and bob@shop.io."),
        "sent to [email] and [email]."
    );
    assert_eq!(
        scrubbed("user@localhost and @mentions"),
        "user@localhost and @mentions"
    );
}

#[test]
fn telephone_numbers_are_taken_out() {
    for (leak, clean) in [
        ("call +86 138 0013 8000 now", "call [phone] now"),
        ("call 13800138000 now", "call [phone] now"),
        ("call (415) 555-2671 now", "call [phone] now"),
        ("call +1-202-555-0143 now", "call [phone] now"),
        ("call +44 20 7946 0958 now", "call [phone] now"),
    ] {
        assert_eq!(scrubbed(leak), clean, "{leak}");
    }

    for text in [
        "version 1.2.3 build 20240518",
        "order 12345 of 67890",
        "t=1716400000123",
        "Chrome/126.0.6478.127",
    ] {
        assert_eq!(text::scrub(&[ScrubKind::Phones], text), None, "{text}");
    }
}

#[test]
fn card_numbers_are_taken_out_when_their_check_digit_and_their_start_say_they_are_cards() {
    for card in [
        "4111 1111 1111 1111",
        "4111-1111-1111-1111",
        "4111111111111111",
        "378282246310005",
        "5555 5555 5555 4444",
        "6011111111111117",
    ] {
        assert_eq!(
            scrubbed(&format!("paid with {card} today")),
            "paid with [card] today",
            "{card}"
        );
    }

    for text in [
        "paid with 4111 1111 1111 1112 today",
        "id 1716400000123",
        "order 2024051812345",
        "1234 5678 9012 3456",
    ] {
        assert_eq!(text::scrub(&[ScrubKind::Cards], text), None, "{text}");
    }
}

#[test]
fn identity_numbers_are_taken_out_by_their_check() {
    assert_eq!(scrubbed("id 11010519491231002X ok"), "id [id] ok");
    assert_eq!(scrubbed("ssn 123-45-6789 ok"), "ssn [id] ok");

    for text in [
        "id 110105194912310021 ok",
        "ssn 000-12-3456",
        "ssn 666-12-3456",
        "ssn 900-12-3456",
        "ssn 123-00-6789",
        "ssn 123-45-0000",
    ] {
        assert_eq!(text::scrub(&[ScrubKind::NationalIds], text), None, "{text}");
    }
}

#[test]
fn only_the_kinds_asked_for_are_taken_out() {
    let text = "mail jane@example.com card 4111 1111 1111 1111 password=hunter2";

    assert_eq!(
        text::scrub(&[ScrubKind::Emails], text).as_deref(),
        Some("mail [email] card 4111 1111 1111 1111 password=hunter2")
    );
    assert_eq!(
        text::scrub(&[ScrubKind::Cards, ScrubKind::Secrets], text).as_deref(),
        Some("mail jane@example.com card [card] password=[secret]")
    );
    assert_eq!(text::scrub(&[], text), None);
    assert_eq!(text::scrub(&[ScrubKind::UrlParams], text), None);
}

#[test]
fn the_values_of_sensitive_parameters_and_the_credentials_before_a_host_are_taken_out_of_an_address()
 {
    for (leak, clean) in [
        (
            "https://shop.example.com/cb?code=abc123&state=xyz&page=2",
            "https://shop.example.com/cb?code=[redacted]&state=xyz&page=2",
        ),
        (
            "https://user:pa55@host.example.com/x?a=1",
            "https://[redacted]@host.example.com/x?a=1",
        ),
        (
            "https://user@host.example.com/x",
            "https://[redacted]@host.example.com/x",
        ),
        (
            "https://app.example.com/#access_token=abc.def&expires_in=3600",
            "https://app.example.com/#access_token=[redacted]&expires_in=3600",
        ),
        (
            "https://app.example.com/#/callback?code=abc&id=5",
            "https://app.example.com/#/callback?code=[redacted]&id=5",
        ),
        (
            "/checkout?session_id=s-123&Email=a%40b.c#top",
            "/checkout?session_id=[redacted]&Email=[redacted]#top",
        ),
        (
            "https://x.example.com/p?api-key=k1&ApiKey=k2&password=p",
            "https://x.example.com/p?api-key=[redacted]&ApiKey=[redacted]&password=[redacted]",
        ),
    ] {
        assert_eq!(url::scrub(leak).as_deref(), Some(clean), "{leak}");
    }

    for address in [
        "https://shop.example.com/products?page=2&sort=price#reviews",
        "https://shop.example.com/",
        "/checkout",
        "https://[redacted]@host.example.com/x?code=[redacted]",
    ] {
        assert_eq!(url::scrub(address), None, "{address}");
    }
}

#[test]
fn an_address_in_the_middle_of_text_is_scrubbed_and_the_position_after_it_stays() {
    assert_eq!(
        url::embedded("at pay (https://cdn.example.com/app.js?key=abc:10:5) and more").as_deref(),
        Some("at pay (https://cdn.example.com/app.js?key=[redacted]:10:5) and more")
    );
    assert_eq!(
        url::embedded("GET https://api.example.com/x?token=abc failed").as_deref(),
        Some("GET https://api.example.com/x?token=[redacted] failed")
    );
    assert_eq!(
        url::embedded("at pay (https://cdn.example.com/app.js:10:5)"),
        None
    );
    assert_eq!(url::embedded("nothing to see"), None);
}

#[test]
fn an_event_is_scrubbed_where_an_sdk_puts_what_people_and_servers_say() {
    let mut event = event(json!({
        "type": "error",
        "view": {
            "url": "https://shop.example.com/users/jane@example.com/orders?token=abc123&page=2",
            "referrer": "https://mail.example.com/?code=zzz",
            "name": "Orders of jane@example.com"
        },
        "error": {
            "message": "Request for jane@example.com failed: password=hunter2",
            "stack": "Error: 4111 1111 1111 1111\n    at pay (https://cdn.example.com/app.js?key=abc:10:5)",
            "causes": [{ "message": "13800138000 unreachable", "type": "NetworkError" }],
            "resource": { "url": "https://api.example.com/pay?access_token=abc" }
        },
        "context": { "note": "call +86 138 0013 8000", "plan": "pro", "nested": { "mail": "bob@shop.io" } },
        "usr": { "id": "u-1", "email": "jane@example.com" }
    }));

    apply(&ScrubKind::ALL, &mut event);

    assert_eq!(
        event["view"]["url"],
        "https://shop.example.com/users/[email]/orders?token=[redacted]&page=2"
    );
    assert_eq!(
        event["view"]["referrer"],
        "https://mail.example.com/?code=[redacted]"
    );
    assert_eq!(event["view"]["name"], "Orders of [email]");
    assert_eq!(
        event["error"]["message"],
        "Request for [email] failed: password=[secret]"
    );
    assert!(
        event["error"]["stack"].as_str().unwrap().starts_with(
            "Error: [card]\n    at pay (https://cdn.example.com/app.js?key=[redacted]:10:5)"
        ),
        "{}",
        event["error"]["stack"]
    );
    assert_eq!(
        event["error"]["causes"][0]["message"],
        "[phone] unreachable"
    );
    assert_eq!(event["error"]["causes"][0]["type"], "NetworkError");
    assert_eq!(
        event["error"]["resource"]["url"],
        "https://api.example.com/pay?access_token=[redacted]"
    );
    assert_eq!(event["context"]["note"], "call [phone]");
    assert_eq!(event["context"]["plan"], "pro");
    assert_eq!(event["context"]["nested"]["mail"], "[email]");

    // What the application sets about the user itself is its own.
    assert_eq!(event["usr"]["email"], "jane@example.com");
    assert_eq!(event["type"], "error");
}

#[test]
fn the_headers_that_carry_credentials_lose_their_values_and_the_others_are_read_as_text() {
    let mut event = event(json!({
        "resource": {
            "url": "https://api.example.com/x",
            "request": { "headers": {
                "Authorization": "Bearer abcdefghijklmnop1234",
                "Cookie": "sid=abc; theme=dark",
                "Accept": "application/json",
                "X-User": "jane@example.com"
            } },
            "response": { "headers": { "Set-Cookie": "sid=abc", "Content-Type": "text/plain" } },
            "graphql": {
                "operation_name": "Login",
                "payload": "mutation { login(email: \"jane@example.com\", password: \"hunter2\") }",
                "variables": "{\"password\":\"hunter2\"}"
            }
        }
    }));

    apply(&ScrubKind::ALL, &mut event);

    let request = &event["resource"]["request"]["headers"];

    assert_eq!(request["Authorization"], "[secret]");
    assert_eq!(request["Cookie"], "[secret]");
    assert_eq!(request["Accept"], "application/json");
    assert_eq!(request["X-User"], "[email]");
    assert_eq!(
        event["resource"]["response"]["headers"]["Set-Cookie"],
        "[secret]"
    );
    assert_eq!(
        event["resource"]["response"]["headers"]["Content-Type"],
        "text/plain"
    );
    assert_eq!(event["resource"]["graphql"]["operation_name"], "Login");
    assert_eq!(
        event["resource"]["graphql"]["payload"],
        "mutation { login(email: \"[email]\", password: \"[secret]\") }"
    );
    assert_eq!(
        event["resource"]["graphql"]["variables"],
        "{\"password\":\"[secret]\"}"
    );

    // Without the kind of secrets the headers are only read as text.
    let mut kept = self::event(json!({
        "resource": { "request": { "headers": { "Authorization": "Bearer abcdefghijklmnop1234" } } }
    }));

    apply(&[ScrubKind::Emails], &mut kept);

    assert_eq!(
        kept["resource"]["request"]["headers"]["Authorization"],
        "Bearer abcdefghijklmnop1234"
    );
}

#[test]
fn an_event_with_nothing_to_take_out_is_not_touched() {
    let original = json!({
        "type": "view",
        "view": { "url": "https://shop.example.com/products?page=2", "name": "Products" },
        "context": { "plan": "pro" }
    });
    let mut event = event(original.clone());

    apply(&ScrubKind::ALL, &mut event);

    assert_eq!(Value::Object(event), original);

    let mut none = self::event(json!({ "view": { "url": "https://a.example/?token=abc" } }));

    apply(&[], &mut none);

    assert_eq!(none["view"]["url"], "https://a.example/?token=abc");
}
