// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Which issue an error belongs to.
//!
//! The id is a hash of what the errors of one problem have in common: the application, where the
//! error comes from, its type, its message with the parts that change from one error to the next
//! taken out, and the first frames of the application's own code. A developer who wants errors
//! grouped some other way sends a fingerprint, and that alone, with the application, decides.
//!
//! The version of the rules is part of the hash. Changing how errors are grouped gives new ids,
//! so the grouping of what was written before is never changed in place.

use std::sync::LazyLock;

use regex::{Captures, Regex};
use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::domain::rum::{MAX_LOCATION_LEN, cut};

/// The version of the grouping rules.
const RULES: &str = "v1";

/// How many frames of the application's own code tell issues apart.
const FRAMES: usize = 3;

/// How much of a message counts, in bytes.
const MESSAGE_BYTES: usize = 512;

/// Hexadecimal digits of an id.
const ID_DIGITS: usize = 16;

static UUID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b")
        .expect("uuid pattern")
});

/// A web address with what follows its path: a query string or a fragment says who asked, not what
/// went wrong.
static URL_TAIL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(https?://[^\s?#"'<>)]*)[?#][^\s"'<>)]*"#).expect("url pattern")
});

static EMAIL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+").expect("email pattern")
});

/// Runs of hexadecimal digits long enough to be a hash, a build id or an address.
static HEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:0x)?[0-9a-f]{8,}\b").expect("hex pattern"));

static NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\d+(?:\.\d+)?").expect("number pattern"));

static SPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("space pattern"));

/// The part of a file name that is a hash of its content: `main.3f9c2a71.js`.
static FILE_HASH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)[.\-_][0-9a-f]{8,}(?:[.\-_]|$)").expect("file pattern"));

/// A message as the same problem words it every time: addresses without their query strings, and
/// numbers, ids, hashes and e-mail addresses given a name instead of their value.
pub fn normalize_message(message: &str) -> String {
    let message = cut(message, MESSAGE_BYTES);
    let message = UUID.replace_all(message, "<uuid>");
    let message = URL_TAIL.replace_all(&message, "$1");
    let message = EMAIL.replace_all(&message, "<email>");

    // A word of letters only is a word, not a hash, however many of them are hexadecimal.
    let message = HEX.replace_all(&message, |found: &Captures<'_>| {
        let text = &found[0];

        if text.bytes().any(|byte| byte.is_ascii_digit()) {
            "<hex>".to_string()
        } else {
            text.to_string()
        }
    });

    let message = NUMBER.replace_all(&message, "<n>");

    SPACE.replace_all(message.trim(), " ").into_owned()
}

/// One frame of a stack, as far as it tells errors apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub file: String,
    pub function: String,
    pub line: Option<u64>,
    pub column: Option<u64>,
    /// Whether the frame was resolved by a source map, a mapping or a symbol file.
    pub restored: bool,
    /// Whether the frame is the application's own code: not a library, a framework or the runtime.
    pub in_app: bool,
}

impl Frame {
    /// What identifies the frame in a hash: the file and the function, and the line only where
    /// there is no function to name.
    fn signature(&self) -> String {
        let file = normalize_file(&self.file);

        if self.function.is_empty() {
            format!("{file}:{}", self.line.unwrap_or_default())
        } else {
            format!("{file}|{}", self.function)
        }
    }

    /// The frame as a place in a list: `file:line · function`.
    pub fn place(&self) -> String {
        let file = display_file(&self.file);
        let mut place = file.clone();

        if let Some(line) = self.line.filter(|_| !file.is_empty()) {
            place.push_str(&format!(":{line}"));
        }

        if !self.function.is_empty() {
            if !place.is_empty() {
                place.push_str(" · ");
            }

            place.push_str(&self.function);
        }

        place
    }
}

/// The frames of an error row, the ones a source map resolved in the place of what the SDK sent.
pub fn frames_of(frames: &Value) -> Vec<Frame> {
    let Some(frames) = frames.as_array() else {
        return Vec::new();
    };

    frames.iter().filter_map(frame).collect()
}

fn frame(value: &Value) -> Option<Frame> {
    let object = value.as_object()?;

    let text = |key: &str| {
        object
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
    };

    let number = |key: &str| object.get(key).and_then(Value::as_u64);

    let restored = text("original_file").is_some()
        || text("original_function").is_some()
        || number("original_line").is_some();

    let (file, function, line, column) = if restored {
        (
            text("original_file").or_else(|| text("file")),
            named(text("original_class"), text("original_function")),
            number("original_line"),
            number("original_column"),
        )
    } else {
        (
            text("file").or_else(|| text("module")),
            named(text("class"), text("function")),
            number("line"),
            number("column"),
        )
    };

    let file = file.unwrap_or_default().to_string();

    if file.is_empty() && function.is_empty() {
        return None;
    }

    let in_app = !is_library(&file);

    Some(Frame {
        file,
        function,
        line,
        column,
        restored,
        in_app,
    })
}

/// Where a frame is: what a page shows of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Place {
    pub file: String,
    pub function: String,
    pub line: Option<u64>,
    pub column: Option<u64>,
}

/// One frame of a stack as the drawer of an issue shows it: what the SDK sent and, when a source
/// map, a mapping or a symbol file resolved it, what it was resolved to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameView {
    pub raw: Place,
    pub restored: Option<Place>,
    /// Whether it is the application's own code, by the place it is shown at.
    pub in_app: bool,
}

/// The frames of an error row as the drawer shows them.
pub fn frame_views(frames: &Value) -> Vec<FrameView> {
    let Some(frames) = frames.as_array() else {
        return Vec::new();
    };

    frames
        .iter()
        .filter_map(|value| {
            let object = value.as_object()?;

            let text = |key: &str| {
                object
                    .get(key)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
            };

            let number = |key: &str| object.get(key).and_then(Value::as_u64);

            let raw = Place {
                file: text("file")
                    .or_else(|| text("module"))
                    .unwrap_or_default()
                    .to_string(),
                function: named(text("class"), text("function")),
                line: number("line"),
                column: number("column"),
            };

            let restored = (text("original_file").is_some()
                || text("original_function").is_some()
                || number("original_line").is_some())
            .then(|| Place {
                file: text("original_file").unwrap_or(&raw.file).to_string(),
                function: named(text("original_class"), text("original_function")),
                line: number("original_line"),
                column: number("original_column"),
            });

            if raw.file.is_empty() && raw.function.is_empty() && restored.is_none() {
                return None;
            }

            let shown = restored
                .as_ref()
                .map_or(raw.file.as_str(), |place| place.file.as_str());
            let in_app = !is_library(shown);

            Some(FrameView {
                raw,
                restored,
                in_app,
            })
        })
        .collect()
}

fn named(class: Option<&str>, function: Option<&str>) -> String {
    match (class, function) {
        (Some(class), Some(function)) => format!("{class}.{function}"),
        (Some(name), None) | (None, Some(name)) => name.to_string(),
        (None, None) => String::new(),
    }
}

/// Whether a file is code the application did not write: a package, a framework or the runtime.
fn is_library(file: &str) -> bool {
    let file = file.to_ascii_lowercase();

    if file.is_empty() {
        return true;
    }

    const FRAGMENTS: &[&str] = &[
        "node_modules/",
        "/webpack/runtime",
        "/vendor/",
        "/.pub-cache/",
        "<anonymous>",
        "[native code]",
        "/system/lib",
        "/usr/lib/",
        "uikitcore",
        "corefoundation",
        "libsystem_",
        "libdispatch",
    ];

    const PREFIXES: &[&str] = &[
        "webpack/",
        "dart:",
        "package:flutter/",
        "java.",
        "javax.",
        "jdk.",
        "sun.",
        "android.",
        "androidx.",
        "dalvik.",
        "com.android.",
        "kotlin.",
        "kotlinx.",
    ];

    matches!(file.as_str(), "native" | "native method")
        || FRAGMENTS.iter().any(|fragment| file.contains(fragment))
        || PREFIXES.iter().any(|prefix| file.starts_with(prefix))
}

/// A file name for hashing: without the host and the query of an address, and without the hash of
/// its content, so that the next build of the same file is the same file.
fn normalize_file(file: &str) -> String {
    let path = path_of(file);

    // The separator that follows a hash stays: `main.3f9c2a71.js` is `main.js`.
    FILE_HASH
        .replace_all(&path, |found: &Captures<'_>| {
            found[0]
                .chars()
                .last()
                .filter(|last| matches!(last, '.' | '-' | '_'))
                .map(String::from)
                .unwrap_or_default()
        })
        .into_owned()
}

/// A file as a list shows it: the path of an address, without a leading slash.
fn display_file(file: &str) -> String {
    path_of(file).trim_start_matches('/').to_string()
}

/// The path of an address, or the text as it is when it is no address.
fn path_of(file: &str) -> String {
    let file = file.split(['?', '#']).next().unwrap_or(file);

    match file.split_once("://") {
        Some((scheme, rest))
            if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") =>
        {
            match rest.split_once('/') {
                Some((_, path)) => format!("/{path}"),
                None => String::new(),
            }
        }
        _ => file.trim_start_matches("./").to_string(),
    }
}

/// The place an error is told by in a list: its first frame of the application's own code, and for
/// a failed request, the request.
pub fn location(row: &Map<String, Value>) -> String {
    let frames = row.get("error.frames").map(frames_of).unwrap_or_default();

    let found = frames
        .iter()
        .find(|frame| frame.in_app)
        .or_else(|| frames.first());

    let place = match found {
        Some(frame) => frame.place(),
        None => request_of(row).unwrap_or_default(),
    };

    cut(&place, MAX_LOCATION_LEN).to_string()
}

/// `POST /api/checkout` for an error that is a failed request.
fn request_of(row: &Map<String, Value>) -> Option<String> {
    let url = text(row, "error.resource.url")?;
    let path = path_of(url);
    let method = text(row, "error.resource.method").unwrap_or_default();

    Some(format!("{method} {path}").trim().to_string())
}

fn text<'a>(row: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    row.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

/// The id of the issue the error row belongs to, or `None` when the row names no application.
pub fn issue_id(row: &Map<String, Value>) -> Option<String> {
    let application = text(row, "application.id")?;
    let mut hash = Hasher::new(application);

    if let Some(fingerprint) = text(row, "error.fingerprint") {
        hash.part("fingerprint");
        hash.part(fingerprint);

        return Some(hash.finish());
    }

    hash.part(text(row, "error.source").unwrap_or_default());
    hash.part(text(row, "error.type").unwrap_or_default());
    hash.part(text(row, "error.category").unwrap_or_default());
    hash.part(&normalize_message(
        text(row, "error.message").unwrap_or_default(),
    ));

    // A failed request is told by what was asked and what came back.
    if let Some(url) = text(row, "error.resource.url") {
        hash.part(text(row, "error.resource.method").unwrap_or_default());
        hash.part(&normalize_message(&path_of(url)));
        hash.part(
            &row.get("error.resource.status_code")
                .and_then(Value::as_i64)
                .unwrap_or_default()
                .to_string(),
        );
    }

    let frames = row.get("error.frames").map(frames_of).unwrap_or_default();

    let own = frames
        .iter()
        .filter(|frame| frame.in_app)
        .take(FRAMES)
        .collect::<Vec<_>>();

    // Errors thrown in nothing but libraries are told by the frames they have.
    let told = if own.is_empty() {
        frames.iter().take(FRAMES).collect()
    } else {
        own
    };

    for frame in told {
        hash.part(&frame.signature());
    }

    Some(hash.finish())
}

/// The id of the issue that holds what an application may no longer open issues for.
pub fn overflow_id(application: &str) -> String {
    let mut hash = Hasher::new(application);

    hash.part("overflow");
    hash.finish()
}

struct Hasher(Sha256);

impl Hasher {
    fn new(application: &str) -> Self {
        let mut hasher = Sha256::new();

        hasher.update(RULES.as_bytes());
        hasher.update([0]);
        hasher.update(application.as_bytes());

        Self(hasher)
    }

    fn part(&mut self, text: &str) {
        self.0.update([0]);
        self.0.update(text.as_bytes());
    }

    fn finish(self) -> String {
        let digest = self.0.finalize();

        digest
            .iter()
            .flat_map(|byte| [byte >> 4, byte & 0x0f])
            .take(ID_DIGITS)
            .map(|nibble| char::from_digit(u32::from(nibble), 16).unwrap_or('0'))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn row(value: Value) -> Map<String, Value> {
        let Value::Object(mut row) = value else {
            panic!("a row is an object");
        };

        row.entry("application.id").or_insert(json!("shop-web"));
        row
    }

    fn id(value: Value) -> String {
        issue_id(&row(value)).expect("an id")
    }

    #[test]
    fn what_changes_from_one_error_to_the_next_is_taken_out_of_the_message() {
        assert_eq!(
            normalize_message("Loading chunk 482 failed."),
            "Loading chunk <n> failed."
        );
        assert_eq!(
            normalize_message("order 3f2b8a10-1111-4222-8333-444455556666 not found"),
            "order <uuid> not found"
        );
        assert_eq!(
            normalize_message("GET https://api.shop.com/cart/17?token=abc&t=99 failed"),
            "GET https://api.shop.com/cart/<n> failed"
        );
        assert_eq!(
            normalize_message("no user ada@example.com"),
            "no user <email>"
        );
        assert_eq!(
            normalize_message("chunk 482.7f3a1c2b.chunk.js"),
            "chunk <n>.<hex>.chunk.js"
        );
        assert_eq!(normalize_message("  spaced \n out  "), "spaced out");
    }

    #[test]
    fn a_word_is_not_a_hash() {
        assert_eq!(
            normalize_message("a deadbeef is not one"),
            "a deadbeef is not one"
        );
        assert_eq!(normalize_message("hash deadbeef01 seen"), "hash <hex> seen");
        assert_eq!(normalize_message("at 0x7fff5fbff8c0"), "at <hex>");
    }

    #[test]
    fn the_same_problem_has_the_same_id_whatever_its_numbers() {
        let one = id(json!({
            "error.source": "source", "error.type": "ChunkLoadError",
            "error.message": "Loading chunk 482 failed.",
        }));
        let other = id(json!({
            "error.source": "source", "error.type": "ChunkLoadError",
            "error.message": "Loading chunk 17 failed.",
        }));

        assert_eq!(one, other);
        assert_eq!(one.len(), ID_DIGITS);
        assert!(one.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[test]
    fn another_application_type_or_message_is_another_issue() {
        let base = json!({
            "error.source": "source", "error.type": "TypeError", "error.message": "x is undefined",
        });
        let reference = id(base.clone());

        let mut other_app = base.clone();
        other_app["application.id"] = json!("shop-app");

        let mut other_type = base.clone();
        other_type["error.type"] = json!("RangeError");

        let mut other_message = base.clone();
        other_message["error.message"] = json!("y is undefined");

        let mut other_source = base;
        other_source["error.source"] = json!("custom");

        for other in [other_app, other_type, other_message, other_source] {
            assert_ne!(id(other), reference);
        }
    }

    #[test]
    fn a_fingerprint_decides_with_the_application_alone() {
        let first = id(json!({
            "error.fingerprint": "payment-declined", "error.type": "A", "error.message": "one",
        }));
        let second = id(json!({
            "error.fingerprint": "payment-declined", "error.type": "B", "error.message": "two",
        }));
        let third = id(json!({
            "application.id": "shop-app",
            "error.fingerprint": "payment-declined", "error.type": "B", "error.message": "two",
        }));

        assert_eq!(first, second);
        assert_ne!(first, third);
    }

    #[test]
    fn the_first_frames_of_the_applications_own_code_tell_issues_apart() {
        let frames = |function: &str| {
            json!([
                { "file": "https://shop.com/static/js/vendor.js", "function": "react", "line": 1, "original_file": "node_modules/react/index.js", "original_function": "react" },
                { "file": "https://shop.com/static/js/main.3f9c2a71.js", "function": "a", "line": 1, "column": 20113,
                  "original_file": "src/cart/summary.ts", "original_function": function, "original_line": 88 },
            ])
        };

        let base = json!({ "error.type": "TypeError", "error.message": "x", "error.frames": frames("loadCart") });
        let again = json!({ "error.type": "TypeError", "error.message": "x", "error.frames": frames("loadCart") });
        let other = json!({ "error.type": "TypeError", "error.message": "x", "error.frames": frames("renderCart") });

        assert_eq!(id(base.clone()), id(again));
        assert_ne!(id(base), id(other));
    }

    #[test]
    fn a_frame_restored_by_a_source_map_is_told_by_what_it_was_restored_to() {
        let restored = id(json!({
            "error.type": "E", "error.message": "m",
            "error.frames": [{ "file": "https://shop.com/main.aaaaaaaa.js", "function": "a", "line": 1,
                               "original_file": "src/a.ts", "original_function": "go", "original_line": 3 }],
        }));
        let rebuilt = id(json!({
            "error.type": "E", "error.message": "m",
            "error.frames": [{ "file": "https://shop.com/main.bbbbbbbb.js", "function": "q", "line": 9,
                               "original_file": "src/a.ts", "original_function": "go", "original_line": 40 }],
        }));
        let unrestored = id(json!({
            "error.type": "E", "error.message": "m",
            "error.frames": [{ "file": "https://shop.com/main.aaaaaaaa.js", "function": "a", "line": 1 }],
        }));

        assert_eq!(restored, rebuilt);
        assert_ne!(restored, unrestored);
    }

    #[test]
    fn a_file_is_the_same_file_in_the_next_build() {
        assert_eq!(
            normalize_file("https://shop.com/static/js/main.3f9c2a71.js?v=2"),
            "/static/js/main.js"
        );
        assert_eq!(normalize_file("src/cart/summary.ts"), "src/cart/summary.ts");
        assert_eq!(
            normalize_file("https://staging.shop.com/static/js/main.3f9c2a71.js"),
            normalize_file("https://shop.com/static/js/main.9d8e7f6a.js")
        );
    }

    #[test]
    fn libraries_and_the_runtime_are_not_the_applications_code() {
        for file in [
            "node_modules/react-dom/index.js",
            "webpack/runtime/jsonp chunk loading",
            "dart:async",
            "package:flutter/src/widgets/framework.dart",
            "java.lang.Thread",
            "androidx.fragment.app.Fragment",
            "",
        ] {
            assert!(is_library(file), "{file}");
        }

        for file in [
            "src/cart/summary.ts",
            "package:shop/cart/summary.dart",
            "com.shop.cart.Summary",
        ] {
            assert!(!is_library(file), "{file}");
        }
    }

    #[test]
    fn a_failed_request_is_told_by_what_was_asked_and_what_came_back() {
        let request = |status: i64, path: &str| {
            id(json!({
                "error.source": "network", "error.message": "Fetch error",
                "error.resource.method": "POST",
                "error.resource.url": format!("https://shop.com{path}?id=1"),
                "error.resource.status_code": status,
            }))
        };

        assert_eq!(request(500, "/api/checkout"), request(500, "/api/checkout"));
        assert_ne!(request(500, "/api/checkout"), request(502, "/api/checkout"));
        assert_ne!(request(500, "/api/checkout"), request(500, "/api/cart"));
    }

    #[test]
    fn the_place_of_an_error_is_its_first_frame_of_the_applications_own_code() {
        let place = location(&row(json!({
            "error.frames": [
                { "file": "node_modules/react/index.js", "function": "render" },
                { "file": "https://shop.com/static/js/main.js", "function": "a", "line": 1,
                  "original_file": "src/cart/summary.ts", "original_function": "loadCart", "original_line": 88 },
            ],
        })));

        assert_eq!(place, "src/cart/summary.ts:88 · loadCart");

        let request = location(&row(json!({
            "error.resource.method": "POST",
            "error.resource.url": "https://shop.com/api/checkout?a=1",
        })));

        assert_eq!(request, "POST /api/checkout");
        assert_eq!(location(&row(json!({}))), "");
    }

    #[test]
    fn a_stack_is_shown_as_it_was_sent_and_as_it_was_restored() {
        let views = frame_views(&json!([
            { "file": "node_modules/react/index.js", "function": "render", "line": 4 },
            { "file": "https://shop.com/main.js", "function": "a", "line": 1, "column": 20113,
              "original_file": "src/cart/summary.ts", "original_function": "loadCart",
              "original_class": "Cart", "original_line": 88, "original_column": 12 },
            { "bogus": true },
        ]));

        assert_eq!(views.len(), 2);
        assert!(!views[0].in_app);
        assert_eq!(views[0].restored, None);
        assert!(views[1].in_app);
        assert_eq!(views[1].raw.function, "a");

        let restored = views[1].restored.as_ref().unwrap();

        assert_eq!(restored.file, "src/cart/summary.ts");
        assert_eq!(restored.function, "Cart.loadCart");
        assert_eq!(restored.line, Some(88));
        assert!(frame_views(&json!("not frames")).is_empty());
    }

    #[test]
    fn a_row_without_an_application_has_no_issue_and_the_rest_has_one_of_its_own() {
        let mut anonymous = Map::new();

        anonymous.insert("error.message".into(), json!("x"));

        assert_eq!(issue_id(&anonymous), None);
        assert_ne!(overflow_id("shop-web"), overflow_id("shop-app"));
        assert_ne!(
            overflow_id("shop-web"),
            id(json!({ "error.message": "overflow" }))
        );
    }
}
