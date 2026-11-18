// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Public GitHub release discovery. A process-wide cache coalesces browser requests.

mod version;

use std::{
    sync::LazyLock,
    time::{Duration, Instant},
};

use kernel::{Error, Result, time::TimestampMicros};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use version::ProductVersion;

const TTL: Duration = Duration::from_secs(6 * 60 * 60);
const RETRY_DELAY: Duration = Duration::from_secs(5 * 60);
const RELEASES_URL: &str = "https://api.github.com/repos/molesignal/molesignal/releases";
static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(|| Mutex::new(Cache::default()));

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub release_url: Option<String>,
    pub update_available: bool,
    pub checked_at: Option<TimestampMicros>,
    pub status: &'static str,
}

impl Default for UpdateInfo {
    fn default() -> Self {
        Self {
            current_version: env!("MOLESIGNAL_PRODUCT_VERSION").to_owned(),
            latest_version: None,
            release_url: None,
            update_available: false,
            checked_at: None,
            status: "unavailable",
        }
    }
}

#[derive(Default)]
struct Cache {
    info: UpdateInfo,
    attempted_at: Option<Instant>,
}

impl Cache {
    fn fresh(&self, refresh: bool) -> bool {
        let ttl = if refresh || self.info.status != "ok" {
            RETRY_DELAY
        } else {
            TTL
        };

        self.attempted_at.is_some_and(|at| at.elapsed() < ttl)
    }
}

/// Manual refresh respects a five-minute cooldown, including failed attempts.
pub async fn check(refresh: bool) -> UpdateInfo {
    let mut cache = CACHE.lock().await;

    if cache.fresh(refresh) {
        return cache.info.clone();
    }

    let result = tokio::time::timeout(Duration::from_secs(20), fetch_latest()).await;
    cache.attempted_at = Some(Instant::now());

    match result {
        Ok(Ok(release)) => {
            let current = ProductVersion::parse(&cache.info.current_version);
            let latest = ProductVersion::parse(&release.tag_name);
            cache.info.update_available = matches!((latest, current), (Some(a), Some(b)) if a > b);
            cache.info.latest_version = Some(release.tag_name.trim_start_matches('v').to_owned());

            // Construct the link from a validated tag, never an arbitrary URL from upstream.
            cache.info.release_url = Some(format!(
                "https://github.com/molesignal/molesignal/releases/tag/{}",
                release.tag_name
            ));

            cache.info.checked_at = Some(TimestampMicros::now());
            cache.info.status = if current.is_some() {
                "ok"
            } else {
                "unavailable"
            };
        }
        _ => cache.info.status = "unavailable",
    }

    cache.info.clone()
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
}

fn newest(releases: impl IntoIterator<Item = Release>) -> Option<Release> {
    releases
        .into_iter()
        .filter(|r| !r.draft)
        .filter_map(|r| ProductVersion::parse(&r.tag_name).map(|version| (version, r)))
        .max_by_key(|(version, _)| *version)
        .map(|(_, release)| release)
}

async fn fetch_latest() -> Result<Release> {
    let client = reqwest::Client::builder()
        .user_agent("MoleSignal-update-check")
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|_| Error::unavailable("Release client unavailable"))?;

    let mut candidates = Vec::new();

    // Releases are not sorted by product version; examine all pages, with a bounded budget.
    for page in 1..=10 {
        let mut response = client
            .get(format!("{RELEASES_URL}?per_page=100&page={page}"))
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|_| Error::unavailable("GitHub releases unavailable"))?;

        let mut bytes = Vec::new();

        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| Error::unavailable("GitHub release response interrupted"))?
        {
            if bytes.len() + chunk.len() > 4 * 1024 * 1024 {
                return Err(Error::unavailable("GitHub release response too large"));
            }

            bytes.extend_from_slice(&chunk);
        }

        let releases: Vec<Release> = serde_json::from_slice(&bytes)
            .map_err(|_| Error::unavailable("Invalid GitHub release response"))?;
        let complete = releases.len() < 100;

        if let Some(release) = newest(releases) {
            candidates.push(release);
        }

        if complete {
            return newest(candidates)
                .ok_or_else(|| Error::unavailable("No published product releases"));
        }
    }

    Err(Error::unavailable("GitHub release listing incomplete"))
}

#[cfg(test)]
mod tests;
