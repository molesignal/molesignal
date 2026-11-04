// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Taking sensitive things out of an event before anything of it is written. An application
//! chooses the kinds (`RumApplicationSettings::scrub`); an event is scrubbed where the SDK may
//! have put text a person typed or a server said: the addresses it was at and came from, the
//! words of an error, the name of what was clicked, what a request carried. The values the SDK
//! is asked to set (the user, the context) are the application's own and are left as they are,
//! except for the documents that hold free text (the causes of an error, the context, the
//! headers of a request).

#[cfg(test)]
mod tests;
mod text;
mod url;

use serde_json::{Map, Value};

use crate::domain::rum::ScrubKind;

/// The addresses of an event, by the names of the fields along the way.
const ADDRESSES: [&[&str]; 5] = [
    &["view", "url"],
    &["view", "referrer"],
    &["resource", "url"],
    &["error", "resource", "url"],
    &["view", "performance", "lcp", "resource_url"],
];

/// The text of an event.
const TEXTS: [&[&str]; 10] = [
    &["message"],
    &["log", "stack"],
    &["view", "name"],
    &["error", "message"],
    &["error", "stack"],
    &["error", "handling_stack"],
    &["action", "target", "name"],
    &["resource", "graphql", "operation_name"],
    &["resource", "graphql", "payload"],
    &["resource", "graphql", "variables"],
];

/// What an event holds as a document, whose strings are free text.
const DOCUMENTS: [&[&str]; 4] = [
    &["error", "causes"],
    &["context"],
    &["resource", "request", "headers"],
    &["resource", "response", "headers"],
];

/// Headers that carry credentials; their values are taken out whatever they say.
const SECRET_HEADERS: [&str; 7] = [
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
    "x-auth-token",
    "x-csrf-token",
];

const SECRET: &str = "[secret]";

fn field<'a>(event: &'a mut Map<String, Value>, path: &[&str]) -> Option<&'a mut Value> {
    let (first, rest) = path.split_first()?;

    rest.iter()
        .try_fold(event.get_mut(*first)?, |value, name| value.get_mut(*name))
}

/// A string as it reads once the kinds are taken out of it, or `None` where it holds none. An
/// address field is read as an address; in any other string the addresses are looked for.
fn string(kinds: &[ScrubKind], text: &str, address: bool) -> Option<String> {
    let cleaned = if !kinds.contains(&ScrubKind::UrlParams) {
        None
    } else if address {
        url::scrub(text)
    } else if text.contains("://") {
        url::embedded(text)
    } else {
        None
    };

    let current = cleaned.as_deref().unwrap_or(text);

    text::scrub(kinds, current).or(cleaned)
}

/// The strings of a document, whatever depth they are at.
fn document(kinds: &[ScrubKind], value: &mut Value) {
    match value {
        Value::String(text) => {
            if let Some(changed) = string(kinds, text, false) {
                *text = changed;
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| document(kinds, item)),
        Value::Object(fields) => fields.values_mut().for_each(|item| document(kinds, item)),
        _ => {}
    }
}

fn headers(kinds: &[ScrubKind], value: &mut Value) {
    let Value::Object(fields) = value else {
        return;
    };

    for (name, header) in fields.iter_mut() {
        if kinds.contains(&ScrubKind::Secrets)
            && SECRET_HEADERS.contains(&name.to_ascii_lowercase().as_str())
        {
            *header = Value::String(SECRET.into());
        } else {
            document(kinds, header);
        }
    }
}

/// Takes the kinds an application asks for out of an event, in place. An event that holds none
/// of them is not touched.
pub fn apply(kinds: &[ScrubKind], event: &mut Map<String, Value>) {
    if kinds.is_empty() {
        return;
    }

    for (paths, address) in [(&ADDRESSES[..], true), (&TEXTS[..], false)] {
        for path in paths {
            if let Some(Value::String(found)) = field(event, path)
                && let Some(changed) = string(kinds, found, address)
            {
                *found = changed;
            }
        }
    }

    for path in DOCUMENTS {
        let Some(value) = field(event, path) else {
            continue;
        };

        if path.ends_with(&["headers"]) {
            headers(kinds, value);
        } else {
            document(kinds, value);
        }
    }
}
