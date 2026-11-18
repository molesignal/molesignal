// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The main menu's preference: the areas of the product one opened lately, the latest first, and
//! whether the menu's entries open in a new browser tab.

use kernel::{Error, Result};
use serde::Deserialize;
use serde_json::Value;

/// The most areas kept.
const MAX_RECENT: usize = 8;
const MAX_ID_CHARS: usize = 64;

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct MainMenu {
    /// Navigation entries by their id (`web/src/product/ia.ts`), such as `logs`.
    recent: Vec<String>,
    /// Whether an entry opens in a new browser tab rather than in place.
    new_tab: bool,
}

/// An id of the product's navigation: lowercase words joined by `-` or `_`.
fn is_entry_id(id: &str) -> bool {
    (1..=MAX_ID_CHARS).contains(&id.len())
        && id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        })
}

/// Whether `value` is the main menu's preference: at most [`MAX_RECENT`] areas, each once, each an
/// id of the navigation, and whether entries open in a new tab.
pub(super) fn validate_main_menu(value: &Value) -> Result<()> {
    let menu: MainMenu = serde_json::from_value(value.clone())
        .map_err(|error| Error::invalid(format!("main menu: {error}")))?;
    let recent = &menu.recent;

    let shaped = recent.len() <= MAX_RECENT
        && recent
            .iter()
            .enumerate()
            .all(|(index, id)| is_entry_id(id) && !recent[..index].contains(id));

    if !shaped {
        return Err(Error::invalid(format!(
            "main menu: at most {MAX_RECENT} recent areas, each once, each a navigation id"
        )));
    }

    // Read for its kind alone: serde has refused anything but a boolean.
    let _opens_in_new_tab = menu.new_tab;

    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn takes_recent_areas_and_the_new_tab_choice() {
        let value = json!({ "recent": ["logs", "traces", "extend-tables", "status_pages"], "newTab": true });

        assert!(validate_main_menu(&value).is_ok());
        assert!(validate_main_menu(&json!({ "recent": [], "newTab": false })).is_ok());
    }

    #[test]
    fn refuses_an_area_twice_too_many_or_not_an_id() {
        let twice = json!({ "recent": ["logs", "logs"], "newTab": false });
        let many = json!({ "recent": (0..=MAX_RECENT).map(|index| format!("area{index}")).collect::<Vec<_>>(), "newTab": false });

        assert!(validate_main_menu(&twice).is_err());
        assert!(validate_main_menu(&many).is_err());

        for id in ["", "Logs", "/logs", "logs?x", &"a".repeat(MAX_ID_CHARS + 1)] {
            let value = json!({ "recent": [id], "newTab": false });

            assert!(validate_main_menu(&value).is_err(), "{id}");
        }
    }

    #[test]
    fn refuses_another_shape() {
        for value in [
            json!({ "recent": ["logs"] }),
            json!({ "recent": ["logs"], "newTab": "yes" }),
            json!({ "recent": ["logs"], "newTab": false, "pinned": [] }),
            json!(["logs"]),
        ] {
            assert!(validate_main_menu(&value).is_err(), "{value}");
        }
    }
}
