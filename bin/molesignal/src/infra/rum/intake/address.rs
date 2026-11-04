// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The parts of a view or resource URL that pages group by. A mobile view is often just a route or
//! a screen name (`/checkout`, `CheckoutActivity`); such a value is its own path.

use url::Url;

/// What a URL is read as: the host, if it has one, and the path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrlParts {
    pub host: Option<String>,
    pub path: String,
}

impl UrlParts {
    /// The path with every segment that looks like an identifier replaced by `?`, so that
    /// `/users/42/orders/9f1c…` and `/users/43/orders/a02d…` are the same page.
    pub fn path_group(&self) -> String {
        group_path(&self.path)
    }
}

pub fn parts(raw: &str) -> UrlParts {
    let raw = raw.trim();

    if let Ok(url) = Url::parse(raw)
        && url.has_host()
    {
        return UrlParts {
            host: url.host_str().map(str::to_ascii_lowercase),
            path: url.path().to_string(),
        };
    }

    // A route, a screen name or a path without a host: the part before the query is the path.
    let path = raw.split(['?', '#']).next().unwrap_or(raw);

    UrlParts {
        host: None,
        path: path.to_string(),
    }
}

fn group_path(path: &str) -> String {
    path.split('/')
        .map(|segment| if looks_like_id(segment) { "?" } else { segment })
        .collect::<Vec<_>>()
        .join("/")
}

fn looks_like_id(segment: &str) -> bool {
    if segment.is_empty() {
        return false;
    }

    if segment.bytes().all(|byte| byte.is_ascii_digit()) {
        return true;
    }

    if is_uuid(segment) {
        return true;
    }

    if segment.len() >= 8 && segment.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return true;
    }

    // A long token that mixes letters with digits: an opaque identifier, not a word.
    let digits = segment.bytes().filter(u8::is_ascii_digit).count();
    let letters = segment.bytes().filter(u8::is_ascii_alphabetic).count();
    let plain = segment
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));

    segment.len() >= 12 && plain && digits >= 3 && letters >= 3
}

fn is_uuid(segment: &str) -> bool {
    let bytes = segment.as_bytes();

    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                *byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absolute_url_gives_its_host_and_path() {
        let parts = parts("https://Shop.Example.com/checkout/payment?step=2#top");

        assert_eq!(parts.host.as_deref(), Some("shop.example.com"));
        assert_eq!(parts.path, "/checkout/payment");
    }

    #[test]
    fn a_route_or_screen_name_is_its_own_path() {
        let route = parts("/checkout?step=2");
        assert_eq!(route.host, None);
        assert_eq!(route.path, "/checkout");

        let screen = parts("CheckoutActivity");
        assert_eq!(screen.host, None);
        assert_eq!(screen.path, "CheckoutActivity");
    }

    #[test]
    fn identifiers_in_a_path_are_grouped() {
        let parts = parts(
            "https://shop.example.com/users/12345/orders/9f1c2b7e-1d5a-4c6b-9a3e-0a1b2c3d4e5f/items",
        );

        assert_eq!(parts.path_group(), "/users/?/orders/?/items");
    }

    #[test]
    fn words_and_short_segments_stay() {
        for path in [
            "/checkout/payment",
            "/api/v1/cart",
            "/blog/how-to-cache",
            "/assets/app.js",
            "/",
        ] {
            assert_eq!(group_path(path), path);
        }
    }

    #[test]
    fn hex_ids_and_mixed_tokens_are_grouped() {
        assert_eq!(group_path("/o/5f2b8c9a1d3e4f60718293a4"), "/o/?");
        assert_eq!(group_path("/s/ab12cd34ef56gh"), "/s/?");
        assert_eq!(group_path("/s/deadbeef"), "/s/?");
    }

    #[test]
    fn an_unparsable_value_is_kept_as_a_path() {
        let parts = parts("::not a url::");

        assert_eq!(parts.host, None);
        assert_eq!(parts.path, "::not a url::");
    }
}
