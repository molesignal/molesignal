// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Real User Monitoring domain contracts.

mod alert;
mod application;
mod comment;
mod debug_artifact;
pub mod event;
mod issue;
mod playlist;
mod ports;
mod retention;
mod segment;
mod summary;

pub use alert::{
    MAX_PERIOD_SECS, MIN_PERIOD_SECS, RumAlertFilter, RumAlertMeasure, RumAlertMetric,
    RumAlertQuery, RumAlertSearch, RumAlertStatistic,
};
pub use application::{
    DEFAULT_ISSUES_PER_HOUR, MAX_ISSUES_PER_HOUR, MAX_SESSION_SAMPLE_RATE, RumApplication,
    RumApplicationPatch, RumApplicationRepository, RumApplicationSettings,
    RumApplicationSettingsPatch, RumPlatform, RumSdkSettings, ScrubKind, id_from_name, resolve_id,
    validate_name,
};
pub use comment::{
    MAX_COMMENT_LEN, MAX_COMMENTS_PER_SESSION, RumReplayComment, RumReplayCommentRepository,
    validate_comment_body,
};
pub use debug_artifact::{
    DebugArtifactKind, DebugArtifactLookup, DebugArtifactMeta, DebugArtifactRepository,
    DebugArtifactUpsert,
};
pub use issue::{
    MAX_ENVIRONMENT_LEN, MAX_LOCATION_LEN, MAX_MESSAGE_LEN, MAX_VERSION_LEN, RumIssue,
    RumIssueChange, RumIssueEnvironment, RumIssueMoment, RumIssueRepository, RumIssueSighting,
    RumIssueStatus, RumVersionRepository, cut, environment_key, version_key,
};
pub use playlist::{
    MAX_PLAYLIST_DESCRIPTION_LEN, MAX_PLAYLIST_NAME_LEN, MAX_PLAYLIST_SESSIONS, RumPlaylist,
    RumPlaylistEntry, RumPlaylistPatch, RumPlaylistRepository, validate_playlist_description,
    validate_playlist_name, validate_playlist_session,
};
pub use ports::{
    RumAlertPoint, RumAlertReading, RumAlertSample, RumAlertSource, RumEventWriter,
    RumIssueTracker, RumSessionMerger,
};
pub use retention::{
    DEFAULT_FILTER, ERRORS_FILTER, MAX_RETENTION_CONDITIONS, MAX_RETENTION_FILTERS,
    MAX_RETENTION_VALUES, RETENTION_FIELDS, RetentionCondition, RetentionFilter, RetentionOp,
    normalize_filters,
};
pub use segment::{
    MAX_SEGMENT_CONDITIONS, MAX_SEGMENT_VALUES, MAX_SEGMENTS, RumSegment, SEGMENT_FIELDS,
    SEGMENT_USER_PREFIX, normalize_segments,
};
pub use summary::{RumSessionSummary, RumSessionSummaryRepository, RumSummaryChapter};

use crate::shared::{Error, Result};

/// Validate and normalize the stable identifier shared by RUM credentials,
/// events, replay segments, and debug artifacts.
pub fn validate_application_id(value: &str) -> Result<&str> {
    let value = value.trim();

    if value.is_empty() || value.len() > 128 {
        return Err(Error::invalid(
            "application_id must contain between 1 and 128 characters",
        ));
    }

    if !value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "._:-".contains(character))
    {
        return Err(Error::invalid(
            "application_id may only contain letters, digits, '.', '_', ':', and '-'",
        ));
    }

    Ok(value)
}

/// Canonicalize common ABI spellings used by Android, Apple, and Flutter.
pub fn normalize_architecture(value: &str) -> String {
    let value = value.trim().to_ascii_lowercase();

    match value.as_str() {
        "aarch64" | "arm64-v8a" | "android-arm64" | "ios-arm64" => "arm64".into(),
        "armeabi-v7a" | "armv7" | "armv7a" | "android-arm" | "ios-arm" => "arm".into(),
        "amd64" | "x64" | "android-x64" | "ios-x64" => "x86_64".into(),
        "i386" | "i686" | "android-x86" | "ios-x86" => "x86".into(),
        _ => value,
    }
}

/// UUIDs and ELF build IDs are compared without presentation punctuation.
pub fn normalize_debug_id(value: &str) -> String {
    let value = value
        .trim()
        .trim_start_matches('{')
        .trim_end_matches('}')
        .to_ascii_lowercase();

    let value = value.strip_prefix("0x").unwrap_or(&value);
    let compact = value.replace('-', "");

    if !compact.is_empty() && compact.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        compact
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn application_id_is_bounded_and_path_safe() {
        assert_eq!(
            validate_application_id("checkout-ios").unwrap(),
            "checkout-ios"
        );
        assert!(validate_application_id("../checkout").is_err());
        assert!(validate_application_id("").is_err());
    }

    #[test]
    fn mobile_build_identifiers_use_canonical_forms() {
        assert_eq!(normalize_architecture("arm64-v8a"), "arm64");
        assert_eq!(normalize_architecture("AMD64"), "x86_64");
        assert_eq!(
            normalize_debug_id("{AABBCCDD-EEFF-0011-2233-445566778899}"),
            "aabbccddeeff00112233445566778899"
        );
        assert_eq!(normalize_debug_id("custom-build"), "custom-build");
    }
}
