// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Credentials and personal data in an address: the user and password before the host, and the
//! values of the parameters of the query and of the fragment that carry them. The names of the
//! parameters stay, so that an address still says what it was asked.

use std::sync::LazyLock;

use regex::{Captures, Regex};

const REDACTED: &str = "[redacted]";

/// An address in the middle of text: from its scheme to the first space, quote or bracket.
static EMBEDDED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"[A-Za-z][A-Za-z0-9+.-]*://[^\s"'<>()\[\]]+"#)
        .expect("the pattern of an address is a valid expression")
});

/// Where in a file a stack says a thing happened: `:10:5` at the end of an address.
static POSITION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r":\d+(?::\d+)?$").expect("the pattern of a position is a valid expression")
});

/// Parameters that are sensitive by their whole name.
const NAMES: &[&str] = &[
    "code", "key", "sig", "sid", "otp", "pwd", "pass", "ticket", "email", "mail", "phone",
    "mobile", "tel", "ssn", "card", "cc", "cvv", "cvc", "iban",
];

/// Words that make a parameter sensitive wherever they are in its name.
const WORDS: &[&str] = &[
    "token",
    "secret",
    "password",
    "passwd",
    "passphrase",
    "auth",
    "signature",
    "apikey",
    "api_key",
    "api-key",
    "credential",
    "jwt",
    "session",
];

fn sensitive(name: &str) -> bool {
    let name = name.to_ascii_lowercase();

    NAMES.contains(&name.as_str()) || WORDS.iter().any(|word| name.contains(word))
}

/// `name=value&name=value` with the values of the sensitive parameters taken out, or `None`
/// where there is nothing to take out. A name may come after a route (`#/orders?token=abc`).
fn pairs(text: &str) -> Option<String> {
    let mut changed = false;

    let parts = text
        .split('&')
        .map(|part| match part.split_once('=') {
            Some((name, value))
                if !value.is_empty()
                    && value != REDACTED
                    && sensitive(name.rsplit(['?', '/']).next().unwrap_or(name)) =>
            {
                changed = true;

                format!("{name}={REDACTED}")
            }
            _ => part.to_string(),
        })
        .collect::<Vec<_>>();

    changed.then(|| parts.join("&"))
}

/// The address with the user and password before the host taken out: `https://user:secret@host/`
/// becomes `https://[redacted]@host/`.
fn user_info(address: &str) -> Option<String> {
    let start = address.find("://")? + 3;
    let end = address[start..]
        .find(['/', '?', '#'])
        .map_or(address.len(), |at| start + at);
    let at = address[start..end].rfind('@')? + start;

    (address[start..at] != *REDACTED)
        .then(|| format!("{}{REDACTED}{}", &address[..start], &address[at..]))
}

/// The address with the credentials and the values of the sensitive parameters taken out, or
/// `None` where there is nothing to take out.
pub fn scrub(original: &str) -> Option<String> {
    let hidden = user_info(original);
    let current = hidden.as_deref().unwrap_or(original);

    let (rest, fragment) = current
        .split_once('#')
        .map_or((current, None), |(rest, fragment)| (rest, Some(fragment)));
    let (path, query) = rest
        .split_once('?')
        .map_or((rest, None), |(path, query)| (path, Some(query)));

    let mut rebuilt = path.to_string();

    if let Some(query) = query {
        rebuilt.push('?');
        rebuilt.push_str(&pairs(query).unwrap_or_else(|| query.to_string()));
    }

    // The fragment of an address that came back from a login holds tokens as a query does.
    if let Some(fragment) = fragment {
        let taken = if fragment.contains('=') {
            pairs(fragment)
        } else {
            None
        };

        rebuilt.push('#');
        rebuilt.push_str(&taken.unwrap_or_else(|| fragment.to_string()));
    }

    (rebuilt != original).then_some(rebuilt)
}

/// The text with the addresses in it scrubbed, or `None` where there is nothing to take out. The
/// position a stack gives after an address is not part of it.
pub fn embedded(text: &str) -> Option<String> {
    let mut changed = false;

    let rewritten = EMBEDDED.replace_all(text, |found: &Captures<'_>| {
        let whole = &found[0];
        let (address, position) = POSITION
            .find(whole)
            .map_or((whole, ""), |at| whole.split_at(at.start()));

        match scrub(address) {
            Some(clean) => {
                changed = true;

                format!("{clean}{position}")
            }
            None => whole.to_string(),
        }
    });

    changed.then(|| rewritten.into_owned())
}
