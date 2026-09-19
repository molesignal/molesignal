// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! RUM event metadata and stack-frame classification.

use serde_json::Value;

use crate::{
    domain::rum::{DebugArtifactKind, normalize_architecture, normalize_debug_id},
    shared::{Error, Result},
};

pub(super) struct EventMetadata {
    pub(super) application_id: String,
    pub(super) service: String,
    pub(super) release: String,
    pub(super) platform: String,
    pub(super) architecture: Option<String>,
    pub(super) debug_id: Option<String>,
    pub(super) flutter: bool,
}

impl EventMetadata {
    pub(super) fn from_event(event: &Value) -> Result<Self> {
        let application_id = string_at(event, &["/application", "/application_id"])
            .ok_or_else(|| Error::invalid("RUM application is missing"))?;
        let service = string_at(event, &["/service"])
            .ok_or_else(|| Error::invalid("RUM service is missing"))?;
        let release = string_at(event, &["/version", "/release"])
            .ok_or_else(|| Error::invalid("RUM version/release is missing"))?;
        let runtime = string_at(
            event,
            &[
                "/runtime",
                "/runtime/name",
                "/sdk/name",
                "/sdk_name",
                "/context/runtime/name",
            ],
        )
        .unwrap_or_default()
        .to_ascii_lowercase();
        let flutter = runtime.contains("flutter")
            || string_at(event, &["/framework"])
                .is_some_and(|value| value.eq_ignore_ascii_case("flutter"));
        let platform = normalize_platform(
            string_at(
                event,
                &[
                    "/platform",
                    "/os",
                    "/os/name",
                    "/device/os",
                    "/context/platform",
                ],
            )
            .unwrap_or(if flutter { "flutter" } else { "web" }),
        );
        Ok(Self {
            application_id: application_id.to_string(),
            service: service.to_string(),
            release: release.to_string(),
            platform,
            architecture: string_at(
                event,
                &[
                    "/architecture",
                    "/device/architecture",
                    "/context/architecture",
                ],
            )
            .map(normalize_architecture),
            debug_id: string_at(
                event,
                &["/debug_id", "/build/debug_id", "/context/debug_id"],
            )
            .map(normalize_debug_id),
            flutter,
        })
    }
}

pub(super) struct FramePlan {
    pub(super) kind: DebugArtifactKind,
    pub(super) filename: Option<String>,
    pub(super) debug_id: Option<String>,
    pub(super) line: Option<u32>,
    pub(super) column: Option<u32>,
    pub(super) class_name: Option<String>,
    pub(super) function: Option<String>,
    pub(super) address: Option<u64>,
}

impl FramePlan {
    pub(super) fn from_frame(frame: &Value, event: &EventMetadata) -> Option<Self> {
        let address = address_at(
            frame,
            &[
                "/relative_address",
                "/instruction_addr",
                "/instruction_address",
                "/address",
                "/pc",
            ],
        );
        if let Some(mut address) = address {
            if frame.get("relative_address").is_none()
                && let Some(image_address) = address_at(
                    frame,
                    &[
                        "/image_addr",
                        "/image_address",
                        "/load_addr",
                        "/load_address",
                    ],
                )
                && address >= image_address
            {
                address -= image_address;
            }
            let kind = explicit_native_kind(frame, event).or_else(|| {
                if event.flutter {
                    Some(DebugArtifactKind::FlutterSymbols)
                } else if event.platform == "ios" {
                    Some(DebugArtifactKind::AppleDsym)
                } else if event.platform == "android" {
                    Some(DebugArtifactKind::AndroidNativeSymbols)
                } else {
                    None
                }
            })?;
            return Some(Self {
                kind,
                filename: string_at(
                    frame,
                    &["/module", "/module_name", "/binary", "/image", "/object"],
                )
                .map(basename)
                .map(str::to_string),
                debug_id: string_at(frame, &["/debug_id", "/build_id", "/uuid"])
                    .map(normalize_debug_id),
                line: None,
                column: None,
                class_name: None,
                function: None,
                address: Some(address),
            });
        }

        let class_name = string_at(frame, &["/class", "/class_name"]);
        let function = string_at(frame, &["/function", "/method", "/name"]);
        if event.platform == "android"
            && let (Some(class_name), Some(function)) = (class_name, function)
        {
            return Some(Self {
                kind: DebugArtifactKind::AndroidMapping,
                filename: None,
                debug_id: None,
                line: integer_at(frame, &["/line", "/line_number"]),
                column: None,
                class_name: Some(class_name.to_string()),
                function: Some(function.to_string()),
                address: None,
            });
        }

        let file = string_at(frame, &["/file", "/filename", "/url"])?;
        let line = integer_at(frame, &["/line", "/line_number"])?;
        let looks_javascript = event.platform == "web"
            || file.split(['?', '#']).next().is_some_and(|path| {
                path.ends_with(".js") || path.ends_with(".mjs") || path.ends_with(".cjs")
            });
        if !looks_javascript {
            return None;
        }
        Some(Self {
            kind: DebugArtifactKind::JavascriptSourcemap,
            filename: Some(format!("{}.map", basename(file))),
            debug_id: string_at(frame, &["/debug_id", "/build_id", "/uuid"])
                .map(normalize_debug_id),
            line: Some(line),
            column: Some(integer_at(frame, &["/column", "/column_number"]).unwrap_or(1)),
            class_name: None,
            function: None,
            address: None,
        })
    }
}

fn explicit_native_kind(frame: &Value, event: &EventMetadata) -> Option<DebugArtifactKind> {
    match string_at(frame, &["/artifact_kind", "/symbol_type"])? {
        "flutter_symbols" => Some(DebugArtifactKind::FlutterSymbols),
        "android_native_symbols" if event.platform == "android" => {
            Some(DebugArtifactKind::AndroidNativeSymbols)
        }
        "apple_dsym" if event.platform == "ios" => Some(DebugArtifactKind::AppleDsym),
        _ => None,
    }
}

fn string_at<'a>(value: &'a Value, paths: &[&str]) -> Option<&'a str> {
    paths
        .iter()
        .find_map(|path| value.pointer(path).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn integer_at(value: &Value, paths: &[&str]) -> Option<u32> {
    paths.iter().find_map(|path| {
        value
            .pointer(path)
            .and_then(|value| {
                value
                    .as_u64()
                    .or_else(|| value.as_str()?.parse::<u64>().ok())
            })
            .and_then(|value| u32::try_from(value).ok())
    })
}

fn address_at(value: &Value, paths: &[&str]) -> Option<u64> {
    paths.iter().find_map(|path| {
        let value = value.pointer(path)?;
        value.as_u64().or_else(|| {
            let text = value.as_str()?.trim();
            if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
                u64::from_str_radix(hex, 16).ok()
            } else {
                text.parse()
                    .ok()
                    .or_else(|| u64::from_str_radix(text, 16).ok())
            }
        })
    })
}

fn normalize_platform(value: &str) -> String {
    let value = value.to_ascii_lowercase();
    if value.contains("android") {
        "android".into()
    } else if value.contains("ios") || value.contains("iphone") || value.contains("ipad") {
        "ios".into()
    } else if value.contains("web") || value.contains("browser") {
        "web".into()
    } else {
        value
    }
}

fn basename(value: &str) -> &str {
    value
        .split(['?', '#'])
        .next()
        .unwrap_or(value)
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn native_addresses_are_normalized_against_image_load_address() {
        let event = EventMetadata {
            application_id: "shop".into(),
            service: "mobile".into(),
            release: "1.0.0".into(),
            platform: "ios".into(),
            architecture: Some("arm64".into()),
            debug_id: None,
            flutter: true,
        };
        let plan = FramePlan::from_frame(
            &json!({"instruction_addr": "0x1010", "image_addr": "0x1000"}),
            &event,
        )
        .expect("native frame");
        assert_eq!(plan.kind, DebugArtifactKind::FlutterSymbols);
        assert_eq!(plan.address, Some(0x10));
    }

    #[test]
    fn javascript_filename_ignores_query_string() {
        assert_eq!(basename("https://cdn.example/app.js?v=1"), "app.js");
    }

    #[test]
    fn javascript_frames_preserve_a_canonical_debug_id() {
        let event = EventMetadata {
            application_id: "shop".into(),
            service: "web".into(),
            release: "1.0.0".into(),
            platform: "web".into(),
            architecture: None,
            debug_id: None,
            flutter: false,
        };
        let plan = FramePlan::from_frame(
            &json!({
                "file": "https://cdn.example/app.js",
                "line": 1,
                "column": 2,
                "debug_id": "{AABB-CCDD}"
            }),
            &event,
        )
        .expect("JavaScript frame");
        assert_eq!(plan.filename.as_deref(), Some("app.js.map"));
        assert_eq!(plan.debug_id.as_deref(), Some("aabbccdd"));
    }

    #[test]
    fn native_frames_select_module_and_frame_debug_id() {
        let event = EventMetadata {
            application_id: "shop".into(),
            service: "mobile".into(),
            release: "1.0.0".into(),
            platform: "android".into(),
            architecture: Some("arm64".into()),
            debug_id: Some("event-build".into()),
            flutter: false,
        };
        let plan = FramePlan::from_frame(
            &json!({
                "instruction_addr": "0x1010",
                "image_addr": "0x1000",
                "module": "/data/app/lib/arm64/libcheckout.so",
                "build_id": "AABBCCDD"
            }),
            &event,
        )
        .expect("native frame");
        assert_eq!(plan.filename.as_deref(), Some("libcheckout.so"));
        assert_eq!(plan.debug_id.as_deref(), Some("aabbccdd"));
        assert_eq!(plan.address, Some(0x10));
    }

    #[test]
    fn flutter_native_plugin_frames_can_select_platform_symbols() {
        let event = EventMetadata {
            application_id: "shop".into(),
            service: "mobile".into(),
            release: "1.0.0".into(),
            platform: "android".into(),
            architecture: Some("arm64".into()),
            debug_id: None,
            flutter: true,
        };
        let plan = FramePlan::from_frame(
            &json!({"relative_address": "0x10", "artifact_kind": "android_native_symbols"}),
            &event,
        )
        .expect("native plugin frame");
        assert_eq!(plan.kind, DebugArtifactKind::AndroidNativeSymbols);
    }

    #[test]
    fn native_address_strings_preserve_decimal_and_hex_radix() {
        assert_eq!(address_at(&json!({"pc": "4096"}), &["/pc"]), Some(4096));
        assert_eq!(address_at(&json!({"pc": "0x1000"}), &["/pc"]), Some(4096));
        assert_eq!(address_at(&json!({"pc": "A000"}), &["/pc"]), Some(0xA000));
    }
}
