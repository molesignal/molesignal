// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The JSON of the HTTP API names its keys in camelCase.
//!
//! This reads the source of the HTTP layer and of the agent tools: a serde struct with a field
//! whose name has an underscore has to say `rename_all = "camelCase"`, and a serde enum whose
//! struct variants have such fields `rename_all_fields = "camelCase"`. Values (enum variants,
//! error codes) keep their own spelling. A file whose JSON follows a protocol is listed in
//! [`PROTOCOL`] with the protocol it follows.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::LazyLock,
};

use regex::Regex;

/// Sources whose JSON is someone else's format, and whose.
const PROTOCOL: &[(&str, &str)] = &[
    (
        "api/http/routes/oauth/",
        "OAuth 2.1 and its registration and metadata documents",
    ),
    (
        "api/http/routes/query/prometheus.rs",
        "the Prometheus HTTP API",
    ),
    ("api/http/routes/intake/otlp", "OTLP/HTTP JSON"),
    (
        "api/http/routes/intake/prometheus.rs",
        "Prometheus remote write",
    ),
    (
        "api/http/routes/intake/compat.rs",
        "the Elasticsearch bulk and Loki push APIs",
    ),
    (
        "api/http/routes/intake/connectors.rs",
        "the Kinesis Firehose, Cloudflare and Heroku pushes",
    ),
    (
        "api/http/routes/alerting/intake.rs",
        "the alert formats of the sources that push them",
    ),
    (
        "api/http/routes/notify/callbacks",
        "the card callbacks of the IM platforms",
    ),
    (
        "api/http/routes/notify/callback_auth.rs",
        "the card callbacks of the IM platforms",
    ),
    (
        "api/http/routes/notify/wecom_callback.rs",
        "the WeCom callback",
    ),
    (
        "api/http/routes/agent/im/inbound.rs",
        "the event callbacks of the IM platforms",
    ),
    (
        "api/http/routes/agent/im/wecom_app.rs",
        "the WeCom callback",
    ),
    ("api/http/routes/sso/saml.rs", "the SAML POST binding"),
    (
        "api/http/routes/sso/oauth.rs",
        "the OAuth authorization response",
    ),
    ("api/http/routes/status_pages/feed.rs", "RSS and Atom"),
];

static STRUCT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:pub(?:\([a-z:]+\))?\s+)?(struct|enum)\s+([A-Za-z0-9_]+)").expect("regex")
});

static FIELD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:pub(?:\([a-z:]+\))?\s+)?([a-z][a-z0-9_]*)\s*:[^:]").expect("regex")
});

static INLINE_TESTS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"#\[cfg\(test\)\]\s*(?:pub(?:\([a-z]+\))? )?mod [a-z_0-9]+ \{").expect("regex")
});

fn sources(dir: &Path, found: &mut Vec<(PathBuf, String)>) {
    for entry in fs::read_dir(dir).expect("read the sources") {
        let path = entry.expect("entry").path();

        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs")
            && !path.to_string_lossy().ends_with("tests.rs")
        {
            let text = fs::read_to_string(&path).expect("read source");
            let production = INLINE_TESTS
                .find(&text)
                .map_or(text.as_str(), |tests| &text[..tests.start()])
                .to_owned();

            found.push((path, production));
        }
    }
}

/// The snake_case field names of one item's body, leaving out those that name their own key.
fn snake_fields(body: &[&str]) -> Vec<String> {
    let mut found = Vec::new();
    let mut renamed = false;

    for line in body {
        let trimmed = line.trim();

        if trimmed.starts_with("#[serde(")
            && (trimmed.contains("rename =") || trimmed.contains("flatten"))
        {
            renamed = true;
            continue;
        }

        if let Some(field) = FIELD.captures(line) {
            let name = &field[1];

            if !renamed && name.trim_matches('_').contains('_') {
                found.push(name.to_owned());
            }

            renamed = false;
        }
    }

    found
}

/// Items of `text` that should be camelCase and are not: `(item, fields)`.
fn offenders(text: &str) -> Vec<(String, Vec<String>)> {
    let lines = text.lines().collect::<Vec<_>>();
    let mut found = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index].trim();

        if !(line.starts_with("#[derive(")
            && (line.contains("Serialize") || line.contains("Deserialize")))
        {
            index += 1;
            continue;
        }

        let mut attributes = String::new();
        let mut item = index;

        while item < lines.len() && STRUCT.captures(lines[item]).is_none() {
            attributes.push_str(lines[item]);
            item += 1;
        }

        let Some(captures) = lines.get(item).and_then(|line| STRUCT.captures(line)) else {
            break;
        };
        let (kind, name) = (captures[1].to_owned(), captures[2].to_owned());

        // The body runs to the brace that closes the item's own.
        let mut depth = 0_i32;
        let mut end = item;

        for (offset, line) in lines[item..].iter().enumerate() {
            depth += line.matches('{').count() as i32 - line.matches('}').count() as i32;

            if depth <= 0 && (line.contains('}') || line.trim_end().ends_with(';')) {
                end = item + offset;
                break;
            }
        }

        let fields = snake_fields(&lines[item + 1..=end.max(item)]);
        let required = if kind == "struct" {
            "rename_all = \"camelCase\""
        } else {
            "rename_all_fields = \"camelCase\""
        };

        if !fields.is_empty()
            && !attributes.contains(required)
            && !attributes.contains("transparent")
        {
            found.push((name, fields));
        }

        index = end + 1;
    }

    found
}

#[test]
fn http_json_keys_are_camel_case() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();

    sources(&root.join("api/http"), &mut files);
    sources(&root.join("app/tools"), &mut files);

    let mut problems = Vec::new();

    for (path, text) in files {
        let relative = path
            .strip_prefix(&root)
            .expect("under src")
            .to_string_lossy()
            .replace('\\', "/");

        if PROTOCOL
            .iter()
            .any(|(prefix, _)| relative.starts_with(prefix))
        {
            continue;
        }

        for (item, fields) in offenders(&text) {
            problems.push(format!("{relative}: {item} ({})", fields.join(", ")));
        }
    }

    assert!(
        problems.is_empty(),
        "serde items of the HTTP API with snake_case keys; add `#[serde(rename_all = \"camelCase\")]` \
         (`rename_all_fields` on an enum), or list a protocol file in PROTOCOL:\n{}",
        problems.join("\n")
    );
}

#[test]
fn the_reader_finds_what_is_missing() {
    let text = "#[derive(Serialize)]\nstruct A {\n    pub next_cursor: String,\n}\n\n\
                #[derive(Serialize)]\n#[serde(rename_all = \"camelCase\")]\nstruct B {\n    next_cursor: String,\n}\n\n\
                #[derive(Deserialize)]\nstruct C {\n    #[serde(rename = \"x_y\")]\n    x_y: u8,\n    name: String,\n}\n";

    assert_eq!(
        offenders(text),
        vec![("A".to_owned(), vec!["next_cursor".to_owned()])]
    );
}
