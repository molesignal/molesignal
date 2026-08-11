// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    body::Body,
    extract::{Path, State},
    http::{
        Response,
        header::{CACHE_CONTROL, CONTENT_TYPE},
    },
};

use crate::{
    api::AppState,
    domain::status_page::{StatusPageIncident, StatusPageSnapshot},
    shared::{Error, Result},
};

pub(super) async fn rss(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Response<Body>> {
    let snapshot = state.status_pages.get_public_snapshot(&slug).await?;
    let body = render_rss(&snapshot, &state.platform.external_url);
    Response::builder()
        .status(200)
        .header(CONTENT_TYPE, "application/rss+xml; charset=utf-8")
        .header(
            CACHE_CONTROL,
            "public, max-age=60, stale-while-revalidate=120",
        )
        .body(Body::from(body))
        .map_err(|error| Error::internal(format!("status-page RSS response: {error}")))
}

fn render_rss(snapshot: &StatusPageSnapshot, external_url: &str) -> String {
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
    incidents.truncate(100);

    let items = incidents
        .into_iter()
        .map(|incident| {
            let link = format!("{page_url}/incidents/{}", incident.id);
            let description = incident
                .updates
                .iter()
                .max_by_key(|update| update.created_at)
                .map(|update| update.message.as_str())
                .unwrap_or_default();
            format!(
                "<item><title>{}</title><link>{}</link><guid isPermaLink=\"false\">{}</guid><description>{}</description><pubDate>{}</pubDate></item>",
                xml_escape(&incident.title),
                xml_escape(&link),
                xml_escape(incident.id.as_str()),
                xml_escape(description),
                incident.updated_at.to_datetime().to_rfc2822(),
            )
        })
        .collect::<String>();

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><rss version=\"2.0\"><channel><title>{}</title><link>{}</link><description>{}</description><lastBuildDate>{}</lastBuildDate>{}</channel></rss>",
        xml_escape(&snapshot.page.name),
        xml_escape(&page_url),
        xml_escape(&format!("Service updates for {}", snapshot.page.name)),
        snapshot.updated_at.to_datetime().to_rfc2822(),
        items,
    )
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

    #[test]
    fn xml_escape_covers_markup_and_attribute_delimiters() {
        assert_eq!(
            xml_escape("A&B <down> \"now\" 'soon'"),
            "A&amp;B &lt;down&gt; &quot;now&quot; &apos;soon&apos;"
        );
    }
}
