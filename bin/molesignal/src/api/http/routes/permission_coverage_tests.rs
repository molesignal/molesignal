// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Every endpoint says what it takes, and every permission it names exists.
//!
//! The router gives away nothing about the checks inside its handlers, so this reads the source:
//! a route whose handler carries no permission attribute and no check of its own has to be listed
//! in [`OPEN`], with the reason it needs none. A new endpoint cannot ship without either.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::LazyLock,
};

use regex::Regex;

/// Calls that decide a permission inside a handler, or that hand the decision to a helper.
const CHECKS: &[&str] = &[
    "has_permission(",
    "require_key(",
    "require_any_key(",
    "authorize_resource",
    "require_platform_permission(",
    "require_organization_action(",
    "require(&context",
    "ensure_allowed(",
    "authorize_scope(",
    "ensure_user_scope(",
    "preflight(",
    "require_manage(",
    "set_disabled(",
    "verify_artifact_lease(",
];

/// Endpoints with no permission of their own, and why. An entry ending in `/` covers what lies under
/// it; any other is that one path.
const OPEN: &[(&str, &str)] = &[
    // Reached before anyone has signed in, or by whoever holds a link.
    ("/.well-known/", "public metadata and the ACME challenge"),
    ("/auth/", "signing in, out and up"),
    ("/oauth/", "the OAuth flow authenticates by itself"),
    ("/docs/inbound-mcp", "public documentation"),
    ("/healthz", "probes"),
    ("/readyz", "probes"),
    ("/version", "public"),
    (
        "/metrics",
        "the Prometheus scrape, public in the auth middleware",
    ),
    ("/instance", "what the sign-in page needs"),
    ("/public/", "public shares authenticate by their own token"),
    ("/s/{token}", "a public share link"),
    (
        "/api/v1/public/",
        "public status pages and webhooks authenticate by their own token",
    ),
    ("/api/v1/files/stream/", "a signed download link"),
    (
        "/api/v1/intake/heartbeats/",
        "a heartbeat authenticates by its token",
    ),
    (
        "/_cloudflare",
        "a push connector authenticates by its token",
    ),
    ("/_heroku", "a push connector authenticates by its token"),
    (
        "/_kinesis_firehose",
        "a push connector authenticates by its token",
    ),
    // About the signed-in account itself, in whichever organization.
    ("/me/", "the signed-in account's own settings"),
    ("/iam/capabilities", "what the caller holds"),
    ("/iam/evaluate-batch", "decisions about the caller"),
    (
        "/orgs/{id}/select",
        "switching organization, which membership or a platform grant decides",
    ),
    (
        "/teams",
        "every member sees the teams of the organization, as they do names",
    ),
    (
        "/teams/{id}",
        "every member sees the teams of the organization, as they do names",
    ),
    (
        "/query/promql/capabilities",
        "what the query language supports",
    ),
    (
        "/query/sql/capabilities",
        "what the query language supports",
    ),
];

fn routes_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/api/http/routes")
}

/// The source under `dir`, without its test modules.
fn production_sources_under(dir: &Path, extension: &str) -> BTreeMap<PathBuf, String> {
    fn walk(dir: &Path, extension: &str, found: &mut BTreeMap<PathBuf, String>) {
        for entry in fs::read_dir(dir).expect("read the sources") {
            let path = entry.expect("entry").path();

            if path.is_dir() {
                walk(&path, extension, found);
            } else if path.extension().is_some_and(|actual| actual == extension)
                && !path.to_string_lossy().ends_with("tests.rs")
            {
                let text = fs::read_to_string(&path).expect("read source");

                let production = INLINE_TESTS
                    .find(&text)
                    .map_or(text.as_str(), |tests| &text[..tests.start()])
                    .to_owned();

                found.insert(path, production);
            }
        }
    }

    let mut found = BTreeMap::new();
    walk(dir, extension, &mut found);
    found
}

fn production_sources() -> BTreeMap<PathBuf, String> {
    production_sources_under(&routes_dir(), "rs")
}

#[derive(Debug, Clone)]
struct Handler {
    file: PathBuf,
    declared: bool,
    body: String,
}

static FUNCTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[ \t]*(?:pub(?:\([a-z: ]+\))? )?async fn ([a-z_0-9]+)\s*[<(]").expect("regex")
});

static ATTRIBUTE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"#\[(?:resource_)?permission\(").expect("regex"));

static ITEM_END: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[ \t]*\}[ \t]*$").expect("regex"));

static INLINE_TESTS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"#\[cfg\(test\)\]\s*(?:pub(?:\([a-z]+\))? )?mod [a-z_0-9]+ \{").expect("regex")
});

static KEY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""([a-z_]+(?:\.[a-z_]+)+)""#).expect("regex"));

static ROUTE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\.route\(\s*"([^"]+)"\s*,"#).expect("regex"));

static METHOD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|[\s.:])(get|post|put|patch|delete)\(\s*([A-Za-z_:][A-Za-z0-9_:]*)")
        .expect("regex")
});

fn handlers(sources: &BTreeMap<PathBuf, String>) -> BTreeMap<String, Vec<Handler>> {
    let mut found: BTreeMap<String, Vec<Handler>> = BTreeMap::new();

    for (file, text) in sources {
        let functions = FUNCTION
            .captures_iter(text)
            .map(|capture| {
                let whole = capture.get(0).expect("match");
                (whole.start(), capture[1].to_owned())
            })
            .collect::<Vec<_>>();

        for (index, (start, name)) in functions.iter().enumerate() {
            let end = functions
                .get(index + 1)
                .map_or(text.len(), |(next, _)| *next);

            found.entry(name.clone()).or_default().push(Handler {
                file: file.clone(),
                declared: declares_permission(&text[..*start]),
                body: text[*start..end].to_owned(),
            });
        }
    }

    found
}

/// Whether the function that starts where `before` ends carries a permission attribute: the
/// attributes are what lies between it and the end of the previous item.
fn declares_permission(before: &str) -> bool {
    let item = ITEM_END
        .find_iter(before)
        .last()
        .map_or(before, |end| &before[end.end()..]);

    ATTRIBUTE.is_match(item)
}

fn balanced_end(text: &str, from: usize) -> usize {
    let mut depth = 1;

    for (offset, byte) in text[from..].bytes().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }

        if depth == 0 {
            return from + offset;
        }
    }

    text.len()
}

fn resolve<'a>(
    handlers: &'a BTreeMap<String, Vec<Handler>>,
    file: &Path,
    written: &str,
) -> Vec<&'a Handler> {
    let name = written.rsplit("::").next().unwrap_or(written);
    let candidates = handlers.get(name).map(Vec::as_slice).unwrap_or_default();
    let qualifier = written.rsplit("::").nth(1);

    if let Some(qualifier) = qualifier {
        let named = candidates
            .iter()
            .filter(|handler| {
                handler
                    .file
                    .with_extension("")
                    .components()
                    .any(|part| part.as_os_str() == qualifier)
            })
            .collect::<Vec<_>>();

        if !named.is_empty() {
            return named;
        }
    }

    let same_file = candidates
        .iter()
        .filter(|handler| handler.file == file)
        .collect::<Vec<_>>();

    if !same_file.is_empty() {
        return same_file;
    }

    let directory = file.with_extension("");
    let parent = file.parent().unwrap_or(file);

    let nearby = candidates
        .iter()
        .filter(|handler| handler.file.starts_with(&directory) || handler.file.starts_with(parent))
        .collect::<Vec<_>>();

    if nearby.is_empty() {
        candidates.iter().collect()
    } else {
        nearby
    }
}

fn is_open(path: &str) -> bool {
    OPEN.iter()
        .any(|(listed, _)| path == *listed || (listed.ends_with('/') && path.starts_with(listed)))
}

#[test]
fn every_endpoint_names_what_it_takes_or_says_why_it_takes_nothing() {
    let sources = production_sources();
    let handlers = handlers(&sources);
    let mut undeclared = BTreeSet::new();
    let mut endpoints = 0;

    for (file, text) in &sources {
        for route in ROUTE.captures_iter(text) {
            let path = &route[1];
            let open_paren = route.get(0).expect("match").end();
            let end = balanced_end(text, open_paren);

            for method in METHOD.captures_iter(&text[open_paren..end]) {
                endpoints += 1;

                let resolved = resolve(&handlers, file, &method[2]);

                let checked = resolved.iter().any(|handler| {
                    handler.declared || CHECKS.iter().any(|check| handler.body.contains(check))
                });

                if !checked && !is_open(path) {
                    undeclared.insert(format!(
                        "{} {path} ({})",
                        method[1].to_uppercase(),
                        &method[2]
                    ));
                }
            }
        }
    }

    assert!(
        endpoints > 800,
        "the route scan found only {endpoints} endpoints"
    );

    assert!(
        undeclared.is_empty(),
        "these endpoints name no permission: give them #[permission(..)], or list why not in OPEN:\n{}",
        undeclared.into_iter().collect::<Vec<_>>().join("\n")
    );
}

fn migrations() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/engines/postgres/src/migrations")
}

/// The catalog as the baseline leaves it: what its seeds insert, and the split that replaces part of
/// it. Reads the SQL so that this needs no database.
fn catalog() -> BTreeSet<String> {
    let sql = fs::read_to_string(migrations().join("20260101000001_initial.sql"))
        .expect("read the baseline");

    let seeded = Regex::new(
        r"\(\s*'([a-z_]+(?:\.[a-z_]+)+)',\s*'(?:organization|platform)',\s*'[a-z_]+',\s*'permissions\.",
    )
    .expect("regex");

    let mut keys = seeded
        .captures_iter(&sql)
        .map(|capture| capture[1].to_owned())
        .collect::<BTreeSet<_>>();

    keys.insert("sys.dashboards.read".to_owned());

    let split = sql
        .split("INSERT INTO iam_permission_split")
        .nth(1)
        .and_then(|rest| rest.split(';').next())
        .expect("the split of the catalog");

    let row = Regex::new(r"\('([a-z_]+(?:\.[a-z_]+)+)', '[a-z_]+', ARRAY").expect("regex");
    keys.extend(
        row.captures_iter(split)
            .map(|capture| capture[1].to_owned()),
    );

    let leaving = sql
        .split("DELETE FROM iam_permissions\nWHERE permission_key IN (")
        .nth(1)
        .and_then(|rest| rest.split(')').next())
        .expect("what leaves the catalog");

    for key in leaving.split(',') {
        keys.remove(key.trim().trim_matches('\''));
    }

    keys
}

fn assert_all_known(what: &str, named: impl IntoIterator<Item = (String, String)>, least: usize) {
    let catalog = catalog();
    let mut count = 0;
    let mut unknown = BTreeSet::new();

    for (key, place) in named {
        count += 1;

        if !catalog.contains(&key) {
            unknown.insert(format!("{key} in {place}"));
        }
    }

    assert!(count >= least, "the scan of {what} found only {count} keys");

    assert!(
        unknown.is_empty(),
        "{what} name permissions that are not in the catalog:\n{}",
        unknown.into_iter().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn every_permission_a_handler_names_is_in_the_catalog() {
    let inline = Regex::new(
        r#"(?:has_permission|require_key|require|authorize_resource(?:::<[A-Za-z_:]+>)?)\((?:[^;"]*,\s*)?"([a-z_]+(?:\.[a-z_]+)+)"\)"#,
    )
    .expect("regex");
    let mut named = Vec::new();

    for (file, text) in production_sources() {
        let place = file.display().to_string();

        for open in ATTRIBUTE.find_iter(&text) {
            let attribute = &text[open.end()..balanced_end(&text, open.end())];

            named.extend(
                KEY.captures_iter(attribute)
                    .map(|key| (key[1].to_owned(), place.clone())),
            );
        }

        named.extend(
            inline
                .captures_iter(&text)
                .map(|key| (key[1].to_owned(), place.clone())),
        );
    }

    assert_all_known("handlers", named, 900);
}

#[test]
fn every_permission_a_page_route_takes_is_in_the_catalog() {
    let sql = fs::read_to_string(migrations().join("20260101000003_iam_route_catalog.sql"))
        .expect("read the route baseline");
    let array = Regex::new(r"ARRAY\[([^\]]*)\]").expect("regex");
    let key = Regex::new(r"'([a-z_]+(?:\.[a-z_]+)+)'").expect("regex");

    let named = array.captures_iter(&sql).flat_map(|list| {
        key.captures_iter(&list[1])
            .map(|key| (key[1].to_owned(), "the route baseline".to_owned()))
            .collect::<Vec<_>>()
    });

    assert_all_known("page routes", named, 100);
}

#[test]
fn every_permission_a_tool_asks_for_is_in_the_catalog() {
    let catalog_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/modules/tool-runtime/src/catalog");
    let list = Regex::new(
        r#"(?:&|vec!)\[("[a-z_]+(?:\.[a-z_]+)+"(?:,\s*"[a-z_]+(?:\.[a-z_]+)+")*),?\s*\]"#,
    )
    .expect("regex");
    let mut named = Vec::new();

    for (file, text) in production_sources_under(&catalog_dir, "rs") {
        for permissions in list.captures_iter(&text) {
            named.extend(
                KEY.captures_iter(&permissions[1])
                    .map(|key| (key[1].to_owned(), file.display().to_string())),
            );
        }
    }

    assert_all_known("tools", named, 100);
}

#[test]
fn every_permission_the_web_asks_for_is_in_the_catalog() {
    let web = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/src");
    let asked = Regex::new(r"(?:\bpermission:\s*|\bhasPermission\(\s*)'([a-z_]+(?:\.[a-z_]+)+)'")
        .expect("regex");
    let mut named = Vec::new();

    for extension in ["ts", "tsx"] {
        for (file, text) in production_sources_under(&web, extension) {
            let place = file.display().to_string();

            if place.contains(".test.") {
                continue;
            }

            named.extend(
                asked
                    .captures_iter(&text)
                    .map(|key| (key[1].to_owned(), place.clone())),
            );
        }
    }

    assert_all_known("web pages", named, 100);
}

#[test]
fn every_permission_of_the_catalog_reads_in_both_languages() {
    let catalog = catalog();
    let i18n = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/src/i18n");

    for language in ["en-us", "zh-cn"] {
        let text = fs::read_to_string(i18n.join(language).join("iam.json")).expect("read copy");
        let copy: serde_json::Value = serde_json::from_str(&text).expect("parse copy");

        for key in &catalog {
            let slug = key.replace('.', "_");

            assert!(
                copy["permissions"][&slug].is_string(),
                "{language} has no name for {key}"
            );
            assert!(
                copy["permissions_hint"][&slug].is_string(),
                "{language} does not say what {key} allows"
            );
        }
    }
}
