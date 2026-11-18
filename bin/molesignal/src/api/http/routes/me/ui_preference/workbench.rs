// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The workbench's recent visits: the dashboards, services, incidents and the like one opened, the
//! latest first, each once.

use kernel::{Error, Result};
use serde::Deserialize;
use serde_json::Value;

/// The most visits kept.
const MAX_VISITS: usize = 12;
const MAX_ID_CHARS: usize = 255;
const MAX_LABEL_CHARS: usize = 200;
const MAX_PATH_CHARS: usize = 2_048;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecentVisits {
    visits: Vec<RecentVisit>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RecentVisit {
    /// What was opened, such as `dashboard` or `incident`.
    kind: String,
    id: String,
    label: String,
    /// Where it opens: a path inside the product, never another site.
    path: String,
    visited_at: i64,
}

impl RecentVisit {
    fn shaped(&self) -> bool {
        (1..=32).contains(&self.kind.len())
            && self
                .kind
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
            && (1..=MAX_ID_CHARS).contains(&self.id.chars().count())
            && (1..=MAX_LABEL_CHARS).contains(&self.label.chars().count())
            && self.path.starts_with('/')
            && !self.path.starts_with("//")
            && self.path.chars().count() <= MAX_PATH_CHARS
            && self
                .path
                .chars()
                .all(|character| character != '\\' && !character.is_control())
            && self.visited_at > 0
    }
}

/// Whether `value` is a list of recent visits: at most [`MAX_VISITS`], each place once, each on a
/// path inside the product.
pub(super) fn validate_recent_visits(value: &Value) -> Result<()> {
    let recent: RecentVisits = serde_json::from_value(value.clone())
        .map_err(|error| Error::invalid(format!("recent visits: {error}")))?;
    let visits = &recent.visits;

    let shaped = visits.len() <= MAX_VISITS
        && visits.iter().enumerate().all(|(index, visit)| {
            visit.shaped()
                && !visits[..index]
                    .iter()
                    .any(|seen| seen.kind == visit.kind && seen.id == visit.id)
        });

    if !shaped {
        return Err(Error::invalid(format!(
            "recent visits: at most {MAX_VISITS}, each place once, on a path inside the product"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn visit(kind: &str, id: &str, path: &str) -> Value {
        json!({ "kind": kind, "id": id, "label": "Checkout", "path": path, "visitedAt": 1 })
    }

    #[test]
    fn takes_places_inside_the_product() {
        let value = json!({ "visits": [
            visit("dashboard", "d1", "/dashboards/d1"),
            visit("incident", "i1", "/alerts/incidents/i1"),
        ] });

        assert!(validate_recent_visits(&value).is_ok());
    }

    #[test]
    fn refuses_a_path_that_leaves_the_product() {
        for path in [
            "https://example.com",
            "//example.com",
            "/\\example.com",
            "dashboards/d1",
        ] {
            let value = json!({ "visits": [visit("dashboard", "d1", path)] });

            assert!(validate_recent_visits(&value).is_err(), "{path}");
        }
    }

    #[test]
    fn refuses_the_same_place_twice_and_too_many() {
        let twice = json!({ "visits": [
            visit("dashboard", "d1", "/dashboards/d1"),
            visit("dashboard", "d1", "/dashboards/d1"),
        ] });
        let many = json!({ "visits": (0..=MAX_VISITS)
            .map(|index| visit("dashboard", &format!("d{index}"), "/dashboards"))
            .collect::<Vec<_>>() });

        assert!(validate_recent_visits(&twice).is_err());
        assert!(validate_recent_visits(&many).is_err());
    }
}
