// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The incidents and maintenance of a public status page as a feed, in RSS 2.0
//! and in Atom 1.0. Both carry the same entries: the latest hundred events,
//! each with its most recent update.

use axum::{
    body::Body,
    extract::{Path, State},
    http::{
        Response,
        header::{CACHE_CONTROL, CONTENT_TYPE},
    },
};
use chrono::SecondsFormat;

use crate::{
    api::AppState,
    domain::status_page::{StatusPageIncident, StatusPageSnapshot},
    shared::{Error, Result, time::TimestampMicros},
};

/// The most events a feed carries.
const FEED_ENTRIES: usize = 100;

pub(super) async fn rss(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Response<Body>> {
    let snapshot = state.status_pages.get_public_snapshot(&slug).await?;
    let body = render_rss(&Feed::of(&snapshot, &state.platform.external_url));

    feed_response("application/rss+xml; charset=utf-8", body)
}

pub(super) async fn atom(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Response<Body>> {
    let snapshot = state.status_pages.get_public_snapshot(&slug).await?;
    let body = render_atom(&Feed::of(&snapshot, &state.platform.external_url));

    feed_response("application/atom+xml; charset=utf-8", body)
}

fn feed_response(content_type: &'static str, body: String) -> Result<Response<Body>> {
    Response::builder()
        .status(200)
        .header(CONTENT_TYPE, content_type)
        .header(
            CACHE_CONTROL,
            "public, max-age=60, stale-while-revalidate=120",
        )
        .body(Body::from(body))
        .map_err(|error| Error::internal(format!("status-page feed response: {error}")))
}

/// What both formats say about a page.
struct Feed {
    title: String,
    page_url: String,
    description: String,
    updated_at: TimestampMicros,
    entries: Vec<FeedEntry>,
}

struct FeedEntry {
    id: String,
    title: String,
    link: String,
    summary: String,
    updated_at: TimestampMicros,
}

impl Feed {
    fn of(snapshot: &StatusPageSnapshot, external_url: &str) -> Self {
        let page_url = format!(
            "{}/status/{}",
            external_url.trim().trim_end_matches('/'),
            snapshot.page.slug
        );

        let mut incidents: Vec<&StatusPageIncident> = snapshot
            .active_incidents
            .iter()
            .chain(snapshot.scheduled_maintenance.iter())
            .chain(snapshot.history.iter())
            .collect();

        incidents.sort_by_key(|incident| std::cmp::Reverse(incident.updated_at));
        incidents.truncate(FEED_ENTRIES);

        let entries = incidents
            .into_iter()
            .map(|incident| FeedEntry {
                id: incident.id.as_str().to_string(),
                title: incident.title.clone(),
                link: format!("{page_url}/incidents/{}", incident.id),
                summary: incident
                    .updates
                    .iter()
                    .max_by_key(|update| update.created_at)
                    .map(|update| update.message.clone())
                    .unwrap_or_default(),
                updated_at: incident.updated_at,
            })
            .collect();

        Self {
            title: snapshot.page.name.clone(),
            description: format!("Service updates for {}", snapshot.page.name),
            page_url,
            updated_at: snapshot.updated_at,
            entries,
        }
    }
}

fn render_rss(feed: &Feed) -> String {
    let items = feed
        .entries
        .iter()
        .map(|entry| {
            format!(
                "<item><title>{}</title><link>{}</link><guid isPermaLink=\"false\">{}</guid><description>{}</description><pubDate>{}</pubDate></item>",
                xml_escape(&entry.title),
                xml_escape(&entry.link),
                xml_escape(&entry.id),
                xml_escape(&entry.summary),
                entry.updated_at.to_datetime().to_rfc2822(),
            )
        })
        .collect::<String>();

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><rss version=\"2.0\"><channel><title>{}</title><link>{}</link><description>{}</description><lastBuildDate>{}</lastBuildDate>{}</channel></rss>",
        xml_escape(&feed.title),
        xml_escape(&feed.page_url),
        xml_escape(&feed.description),
        feed.updated_at.to_datetime().to_rfc2822(),
        items,
    )
}

/// Atom names the feed and its entries with identifiers that never change: the
/// page address, and a URN for each event, so a reader keeps an entry it has
/// seen.
fn render_atom(feed: &Feed) -> String {
    let entries = feed
        .entries
        .iter()
        .map(|entry| {
            format!(
                "<entry><id>urn:molesignal:status-page-event:{}</id><title>{}</title><link rel=\"alternate\" type=\"text/html\" href=\"{}\"/><updated>{}</updated><summary>{}</summary></entry>",
                xml_escape(&entry.id),
                xml_escape(&entry.title),
                xml_escape(&entry.link),
                atom_time(entry.updated_at),
                xml_escape(&entry.summary),
            )
        })
        .collect::<String>();

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><feed xmlns=\"http://www.w3.org/2005/Atom\"><id>{}</id><title>{}</title><subtitle>{}</subtitle><link rel=\"alternate\" type=\"text/html\" href=\"{}\"/><updated>{}</updated><author><name>{}</name></author>{}</feed>",
        xml_escape(&feed.page_url),
        xml_escape(&feed.title),
        xml_escape(&feed.description),
        xml_escape(&feed.page_url),
        atom_time(feed.updated_at),
        xml_escape(&feed.title),
        entries,
    )
}

fn atom_time(at: TimestampMicros) -> String {
    at.to_datetime().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed() -> Feed {
        Feed {
            title: "Acme & Co".into(),
            page_url: "https://ms.example.com/status/acme".into(),
            description: "Service updates for Acme & Co".into(),
            updated_at: TimestampMicros(1_779_616_080_000_000),
            entries: vec![FeedEntry {
                id: "evt-1".into(),
                title: "API <errors>".into(),
                link: "https://ms.example.com/status/acme/incidents/evt-1".into(),
                summary: "Recovered".into(),
                updated_at: TimestampMicros(1_779_616_080_000_000),
            }],
        }
    }

    #[test]
    fn xml_escape_covers_markup_and_attribute_delimiters() {
        assert_eq!(
            xml_escape("A&B <down> \"now\" 'soon'"),
            "A&amp;B &lt;down&gt; &quot;now&quot; &apos;soon&apos;"
        );
    }

    #[test]
    fn both_formats_carry_the_same_escaped_entries() {
        let rss = render_rss(&feed());
        assert!(rss.contains("<rss version=\"2.0\">"));
        assert!(rss.contains("<title>API &lt;errors&gt;</title>"));
        assert!(rss.contains("<guid isPermaLink=\"false\">evt-1</guid>"));

        let atom = render_atom(&feed());
        assert!(atom.contains("<feed xmlns=\"http://www.w3.org/2005/Atom\">"));
        assert!(atom.contains("<title>Acme &amp; Co</title>"));
        assert!(atom.contains("<id>urn:molesignal:status-page-event:evt-1</id>"));
        assert!(atom.contains("<updated>2026-05-24T09:48:00Z</updated>"));
        assert!(atom.contains("<author><name>Acme &amp; Co</name></author>"));
    }
}
