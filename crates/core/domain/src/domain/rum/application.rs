// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The applications that send RUM data. An application is created first, then SDKs are given its
//! client token; an event can only be written to the application its token is bound to.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{
    retention::{RetentionFilter, normalize_filters},
    segment::{RumSegment, normalize_segments},
    validate_application_id,
};
use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

const MAX_NAME_LEN: usize = 128;
const MAX_ID_LEN: usize = 64;

/// How many new issues an application opens in an hour unless it is set otherwise.
pub const DEFAULT_ISSUES_PER_HOUR: u32 = 100;
/// The most an application may be allowed to open in an hour.
pub const MAX_ISSUES_PER_HOUR: u32 = 10_000;

/// The most sessions an application can be set to keep: all of them.
pub const MAX_SESSION_SAMPLE_RATE: u32 = 100;

/// A kind of sensitive text an application removes from its events before anything is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrubKind {
    /// Tokens, keys, passwords and credentials in text and in the headers of a request.
    Secrets,
    /// Email addresses.
    Emails,
    /// Telephone numbers.
    Phones,
    /// Payment card numbers.
    Cards,
    /// National identity numbers.
    NationalIds,
    /// The values of the parameters of a URL that carry credentials or personal data, and the
    /// credentials before the host.
    UrlParams,
}

impl ScrubKind {
    /// Every kind, in the order they are shown and stored.
    pub const ALL: [Self; 6] = [
        Self::Secrets,
        Self::Emails,
        Self::Phones,
        Self::Cards,
        Self::NationalIds,
        Self::UrlParams,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Secrets => "secrets",
            Self::Emails => "emails",
            Self::Phones => "phones",
            Self::Cards => "cards",
            Self::NationalIds => "national_ids",
            Self::UrlParams => "url_params",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == name)
    }

    /// The kinds named, each once, in the order of [`Self::ALL`].
    pub fn normalized(kinds: &[Self]) -> Vec<Self> {
        Self::ALL
            .into_iter()
            .filter(|kind| kinds.contains(kind))
            .collect()
    }
}

/// What kind of client the application is. It decides which SDK snippet the application page
/// shows and nothing else: an event says for itself what sent it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RumPlatform {
    #[serde(rename = "browser")]
    Browser,
    #[serde(rename = "android")]
    Android,
    #[serde(rename = "ios")]
    Ios,
    #[serde(rename = "flutter")]
    Flutter,
    #[serde(rename = "react-native")]
    ReactNative,
    #[serde(rename = "other")]
    Other,
}

/// What the settings of an application decide for an SDK that asks at start
/// (`remoteConfiguration`). Left out, the SDK goes by what the page gave it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct RumSdkSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_sample_rate: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_replay_sample_rate: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_privacy_level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_user_interactions: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forward_console_logs: Option<Vec<String>>,
}

/// The console methods an SDK can forward.
const CONSOLE_LEVELS: [&str; 5] = ["debug", "log", "info", "warn", "error"];

impl RumSdkSettings {
    pub fn validate(&self) -> Result<()> {
        for rate in [self.session_sample_rate, self.session_replay_sample_rate] {
            if rate.is_some_and(|rate| rate > MAX_SESSION_SAMPLE_RATE) {
                return Err(Error::invalid(format!(
                    "a sample rate of the SDK must be between 0 and {MAX_SESSION_SAMPLE_RATE}"
                )));
            }
        }

        self.default_privacy_level
            .as_deref()
            .map_or(Ok(()), validate_privacy)?;

        if let Some(levels) = &self.forward_console_logs
            && let Some(unknown) = levels
                .iter()
                .find(|level| !CONSOLE_LEVELS.contains(&level.as_str()))
        {
            return Err(Error::invalid(format!(
                "`{unknown}` is not a console method the SDK forwards"
            )));
        }

        Ok(())
    }
}

/// How an application's data is collected. Both switches default to on, and what a request leaves
/// out takes its default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RumApplicationSettings {
    /// Keep the client IP address with each event.
    pub collect_ip: bool,
    /// Look the client IP up in the GeoIP database and keep the place with each event. Without the
    /// IP address stored the lookup is still made; only its result is kept.
    pub collect_geo: bool,
    /// The privacy level the application page suggests in the SDK snippet: `mask`,
    /// `mask-user-input` or `allow`.
    pub default_privacy: String,
    /// How many new issues the application opens in an hour. The errors of one that would be
    /// the next are counted together as the rest.
    pub issues_per_hour: u32,
    /// The kinds of sensitive text removed from the URLs, the error text, the names and the
    /// details of the requests of its events before they are stored.
    pub scrub: Vec<ScrubKind>,
    /// The share of the sessions that no filter describes that is kept, in percent. A session is
    /// kept or dropped as a whole, by its id, on top of what the SDK samples.
    pub session_sample_rate: u32,
    /// Shares of the sessions that meet some conditions, the first that describes a session
    /// deciding its share. What no filter describes is kept by `session_sample_rate`.
    pub retention_filters: Vec<RetentionFilter>,
    /// Keep the errors of the sessions that are not kept. Without them a rare error is lost with
    /// its session, and the errors of the application are counted short.
    pub keep_errors: bool,
    /// What an SDK that asks at start goes by.
    pub sdk: RumSdkSettings,
    /// The groups of sessions the analytics pages can be narrowed to.
    pub segments: Vec<RumSegment>,
}

impl Default for RumApplicationSettings {
    fn default() -> Self {
        Self {
            collect_ip: true,
            collect_geo: true,
            default_privacy: "mask".into(),
            issues_per_hour: DEFAULT_ISSUES_PER_HOUR,
            scrub: ScrubKind::ALL.to_vec(),
            session_sample_rate: MAX_SESSION_SAMPLE_RATE,
            retention_filters: Vec::new(),
            keep_errors: true,
            sdk: RumSdkSettings::default(),
            segments: Vec::new(),
        }
    }
}

impl RumApplicationSettings {
    pub fn validate(&self) -> Result<()> {
        self.sdk.validate()?;
        validate_privacy(&self.default_privacy)?;
        validate_session_sample_rate(self.session_sample_rate)?;
        validate_issues_per_hour(self.issues_per_hour)?;
        normalize_filters(self.retention_filters.clone())?;
        normalize_segments(self.segments.clone()).map(|_| ())
    }

    /// The settings as they are stored: checked, and each retention filter and segment named by an
    /// id.
    pub fn normalized(mut self) -> Result<Self> {
        self.validate()?;
        self.retention_filters = normalize_filters(self.retention_filters)?;
        self.segments = normalize_segments(self.segments)?;

        Ok(self)
    }
}

fn validate_session_sample_rate(rate: u32) -> Result<()> {
    if rate > MAX_SESSION_SAMPLE_RATE {
        return Err(Error::invalid(format!(
            "session_sample_rate must be between 0 and {MAX_SESSION_SAMPLE_RATE}"
        )));
    }

    Ok(())
}

fn validate_privacy(level: &str) -> Result<()> {
    if !matches!(level, "mask" | "mask-user-input" | "allow") {
        return Err(Error::invalid(
            "a privacy level must be `mask`, `mask-user-input` or `allow`",
        ));
    }

    Ok(())
}

fn validate_issues_per_hour(limit: u32) -> Result<()> {
    if !(1..=MAX_ISSUES_PER_HOUR).contains(&limit) {
        return Err(Error::invalid(format!(
            "issues_per_hour must be between 1 and {MAX_ISSUES_PER_HOUR}"
        )));
    }

    Ok(())
}

/// The settings a change names. The ones it leaves out keep their value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RumApplicationSettingsPatch {
    pub collect_ip: Option<bool>,
    pub collect_geo: Option<bool>,
    pub default_privacy: Option<String>,
    pub issues_per_hour: Option<u32>,
    pub scrub: Option<Vec<ScrubKind>>,
    pub session_sample_rate: Option<u32>,
    pub retention_filters: Option<Vec<RetentionFilter>>,
    pub keep_errors: Option<bool>,
    pub sdk: Option<RumSdkSettings>,
    pub segments: Option<Vec<RumSegment>>,
}

impl RumApplicationSettingsPatch {
    pub fn validate(&self) -> Result<()> {
        self.sdk.as_ref().map_or(Ok(()), RumSdkSettings::validate)?;

        self.default_privacy
            .as_deref()
            .map_or(Ok(()), validate_privacy)?;

        self.session_sample_rate
            .map_or(Ok(()), validate_session_sample_rate)?;

        self.issues_per_hour
            .map_or(Ok(()), validate_issues_per_hour)?;

        self.retention_filters.as_ref().map_or(Ok(()), |filters| {
            normalize_filters(filters.clone()).map(|_| ())
        })?;

        self.segments.as_ref().map_or(Ok(()), |segments| {
            normalize_segments(segments.clone()).map(|_| ())
        })
    }

    /// The settings after this change.
    pub fn applied_to(&self, current: &RumApplicationSettings) -> RumApplicationSettings {
        RumApplicationSettings {
            collect_ip: self.collect_ip.unwrap_or(current.collect_ip),
            collect_geo: self.collect_geo.unwrap_or(current.collect_geo),
            default_privacy: self
                .default_privacy
                .clone()
                .unwrap_or_else(|| current.default_privacy.clone()),
            issues_per_hour: self.issues_per_hour.unwrap_or(current.issues_per_hour),
            scrub: self
                .scrub
                .as_deref()
                .map_or_else(|| current.scrub.clone(), ScrubKind::normalized),
            session_sample_rate: self
                .session_sample_rate
                .unwrap_or(current.session_sample_rate),
            // A list that was not checked keeps what there was; a change is checked before it is
            // applied.
            retention_filters: self.retention_filters.as_ref().map_or_else(
                || current.retention_filters.clone(),
                |filters| {
                    normalize_filters(filters.clone())
                        .unwrap_or_else(|_| current.retention_filters.clone())
                },
            ),
            keep_errors: self.keep_errors.unwrap_or(current.keep_errors),
            sdk: self.sdk.clone().unwrap_or_else(|| current.sdk.clone()),
            segments: self.segments.as_ref().map_or_else(
                || current.segments.clone(),
                |segments| {
                    normalize_segments(segments.clone())
                        .unwrap_or_else(|_| current.segments.clone())
                },
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RumApplication {
    pub org_id: Id,
    /// The identifier SDKs are configured with; fixed once created.
    pub id: String,
    pub name: String,
    pub platform: RumPlatform,
    pub settings: RumApplicationSettings,
    pub created_by: Option<Id>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

/// The parts of an application that can change afterwards.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RumApplicationPatch {
    pub name: Option<String>,
    pub platform: Option<RumPlatform>,
    pub settings: Option<RumApplicationSettingsPatch>,
}

impl RumApplicationPatch {
    pub fn validate(&self) -> Result<()> {
        if let Some(name) = &self.name {
            validate_name(name)?;
        }

        if let Some(settings) = &self.settings {
            settings.validate()?;
        }

        Ok(())
    }
}

pub fn validate_name(name: &str) -> Result<()> {
    let name = name.trim();

    if name.is_empty() || name.chars().count() > MAX_NAME_LEN {
        return Err(Error::invalid(format!(
            "application name must contain between 1 and {MAX_NAME_LEN} characters"
        )));
    }

    Ok(())
}

/// The identifier an application gets from its name when the caller gives none: lower-case
/// letters and digits, joined by single dashes.
pub fn id_from_name(name: &str) -> String {
    let mut id = String::new();

    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            id.push(character.to_ascii_lowercase());
        } else if !id.is_empty() && !id.ends_with('-') {
            id.push('-');
        }
    }

    let id = id.trim_matches('-');
    let id = &id[..id.len().min(MAX_ID_LEN)];
    let id = id.trim_end_matches('-');

    if id.is_empty() {
        "app".into()
    } else {
        id.into()
    }
}

/// The identifier a new application is created with: the caller's, checked, or one made from the
/// name.
pub fn resolve_id(requested: Option<&str>, name: &str) -> Result<String> {
    match requested.map(str::trim).filter(|value| !value.is_empty()) {
        Some(requested) => {
            let requested = validate_application_id(requested)?;

            if requested.len() > MAX_ID_LEN {
                return Err(Error::invalid(format!(
                    "application id must not be longer than {MAX_ID_LEN} characters"
                )));
            }

            Ok(requested.to_string())
        }
        None => Ok(id_from_name(name)),
    }
}

#[async_trait]
pub trait RumApplicationRepository: Send + Sync {
    /// Creates the application; a taken `(org, id)` is a conflict.
    async fn create(&self, application: RumApplication) -> Result<RumApplication>;

    /// Not found is `Error::NotFound`.
    async fn get(&self, org_id: &Id, id: &str) -> Result<RumApplication>;

    /// The applications of the organization, oldest first.
    async fn list(&self, org_id: &Id) -> Result<Vec<RumApplication>>;

    async fn update(
        &self,
        org_id: &Id,
        id: &str,
        patch: RumApplicationPatch,
    ) -> Result<RumApplication>;

    async fn delete(&self, org_id: &Id, id: &str) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_a_request_leaves_out_take_their_defaults() {
        let settings: RumApplicationSettings =
            serde_json::from_value(serde_json::json!({ "collectIp": false })).unwrap();

        assert!(!settings.collect_ip);
        assert!(settings.collect_geo);
        assert_eq!(settings.default_privacy, "mask");

        let none: RumApplicationSettings = serde_json::from_value(serde_json::json!({})).unwrap();

        assert_eq!(none, RumApplicationSettings::default());
    }

    #[test]
    fn a_change_of_settings_keeps_what_it_does_not_name() {
        let current = RumApplicationSettings {
            collect_ip: false,
            collect_geo: true,
            default_privacy: "allow".into(),
            issues_per_hour: 20,
            scrub: vec![ScrubKind::Secrets],
            session_sample_rate: 25,
            retention_filters: Vec::new(),
            keep_errors: false,
            sdk: RumSdkSettings::default(),
            segments: Vec::new(),
        };
        let change: RumApplicationSettingsPatch =
            serde_json::from_value(serde_json::json!({ "collectGeo": false })).unwrap();

        assert_eq!(
            change.applied_to(&current),
            RumApplicationSettings {
                collect_ip: false,
                collect_geo: false,
                default_privacy: "allow".into(),
                issues_per_hour: 20,
                scrub: vec![ScrubKind::Secrets],
                session_sample_rate: 25,
                retention_filters: Vec::new(),
                keep_errors: false,
                sdk: RumSdkSettings::default(),
                segments: Vec::new(),
            }
        );
        assert_eq!(
            RumApplicationSettingsPatch::default().applied_to(&current),
            current
        );
    }

    #[test]
    fn a_change_is_checked_for_the_privacy_level_it_names() {
        let bad = RumApplicationSettingsPatch {
            default_privacy: Some("everything".into()),
            ..Default::default()
        };

        assert!(bad.validate().is_err());
        assert!(RumApplicationSettingsPatch::default().validate().is_ok());
    }

    #[test]
    fn the_issues_an_hour_are_between_one_and_the_most() {
        let named = |limit| RumApplicationSettingsPatch {
            issues_per_hour: Some(limit),
            ..Default::default()
        };

        assert!(named(0).validate().is_err());
        assert!(named(1).validate().is_ok());
        assert!(named(MAX_ISSUES_PER_HOUR).validate().is_ok());
        assert!(named(MAX_ISSUES_PER_HOUR + 1).validate().is_err());

        let settings = RumApplicationSettings {
            issues_per_hour: 0,
            ..Default::default()
        };

        assert!(settings.validate().is_err());
        assert_eq!(
            RumApplicationSettings::default().issues_per_hour,
            DEFAULT_ISSUES_PER_HOUR
        );
    }

    #[test]
    fn an_application_scrubs_everything_and_keeps_every_session_unless_it_is_told_otherwise() {
        let settings = RumApplicationSettings::default();

        assert_eq!(settings.scrub, ScrubKind::ALL.to_vec());
        assert_eq!(settings.session_sample_rate, MAX_SESSION_SAMPLE_RATE);

        let named: RumApplicationSettings = serde_json::from_value(
            serde_json::json!({ "scrub": ["emails"], "sessionSampleRate": 10 }),
        )
        .unwrap();

        assert_eq!(named.scrub, vec![ScrubKind::Emails]);
        assert_eq!(named.session_sample_rate, 10);
    }

    #[test]
    fn the_kinds_to_scrub_are_kept_once_each_in_their_order_and_none_is_allowed() {
        let change = RumApplicationSettingsPatch {
            scrub: Some(vec![
                ScrubKind::UrlParams,
                ScrubKind::Emails,
                ScrubKind::UrlParams,
            ]),
            ..Default::default()
        };

        assert_eq!(
            change.applied_to(&RumApplicationSettings::default()).scrub,
            vec![ScrubKind::Emails, ScrubKind::UrlParams]
        );

        let none = RumApplicationSettingsPatch {
            scrub: Some(Vec::new()),
            ..Default::default()
        };

        assert!(
            none.applied_to(&RumApplicationSettings::default())
                .scrub
                .is_empty()
        );
        assert_eq!(
            ScrubKind::parse("national_ids"),
            Some(ScrubKind::NationalIds)
        );
        assert_eq!(ScrubKind::parse("everything"), None);
        assert!(
            serde_json::from_value::<RumApplicationSettingsPatch>(
                serde_json::json!({ "scrub": ["everything"] })
            )
            .is_err()
        );
    }

    #[test]
    fn the_share_of_sessions_kept_is_a_percentage() {
        let named = |rate| RumApplicationSettingsPatch {
            session_sample_rate: Some(rate),
            ..Default::default()
        };

        assert!(named(0).validate().is_ok());
        assert!(named(100).validate().is_ok());
        assert!(named(101).validate().is_err());

        let settings = RumApplicationSettings {
            session_sample_rate: 150,
            ..Default::default()
        };

        assert!(settings.validate().is_err());
    }

    fn filter(name: &str, rate: u32) -> RetentionFilter {
        RetentionFilter {
            id: String::new(),
            name: name.into(),
            rate,
            conditions: Vec::new(),
        }
    }

    #[test]
    fn an_application_keeps_the_errors_of_the_sessions_it_drops_and_has_no_filters_unless_told_otherwise()
     {
        let settings = RumApplicationSettings::default();

        assert!(settings.keep_errors);
        assert!(settings.retention_filters.is_empty());

        let named: RumApplicationSettings = serde_json::from_value(serde_json::json!({
            "keepErrors": false,
            "retentionFilters": [{ "name": "Production", "rate": 100 }]
        }))
        .unwrap();

        assert!(!named.keep_errors);
        assert_eq!(named.retention_filters[0].conditions, Vec::new());
    }

    #[test]
    fn a_change_of_filters_is_checked_and_each_filter_is_named_by_an_id() {
        let change = RumApplicationSettingsPatch {
            retention_filters: Some(vec![
                filter("Production", 100),
                filter("Everything else", 10),
            ]),
            keep_errors: Some(false),
            ..Default::default()
        };

        assert!(change.validate().is_ok());

        let applied = change.applied_to(&RumApplicationSettings::default());

        assert!(!applied.keep_errors);
        assert_eq!(
            applied
                .retention_filters
                .iter()
                .map(|filter| filter.id.as_str())
                .collect::<Vec<_>>(),
            ["production", "everything-else"]
        );

        // A list that is not right is refused, and left out when it is applied all the same.
        let bad = RumApplicationSettingsPatch {
            retention_filters: Some(vec![filter("Too much", 101)]),
            ..Default::default()
        };

        assert!(bad.validate().is_err());
        assert_eq!(
            bad.applied_to(&applied).retention_filters,
            applied.retention_filters
        );

        // And one that is not named keeps what there was.
        assert_eq!(
            RumApplicationSettingsPatch::default()
                .applied_to(&applied)
                .retention_filters,
            applied.retention_filters
        );
    }

    #[test]
    fn settings_are_stored_with_their_filters_named() {
        let settings = RumApplicationSettings {
            retention_filters: vec![filter("Beta", 50)],
            ..Default::default()
        };

        assert!(settings.validate().is_ok());
        assert_eq!(
            settings.normalized().unwrap().retention_filters[0].id,
            "beta"
        );

        let bad = RumApplicationSettings {
            retention_filters: vec![filter("A", 1), filter("a", 2)],
            ..Default::default()
        };

        assert!(bad.validate().is_err());
        assert!(bad.normalized().is_err());
    }

    #[test]
    fn ids_are_made_from_names() {
        assert_eq!(id_from_name("Checkout Web"), "checkout-web");
        assert_eq!(id_from_name("  --Shop / iOS (beta)  "), "shop-ios-beta");
        assert_eq!(id_from_name("日本語"), "app");
        assert_eq!(id_from_name(""), "app");
    }

    #[test]
    fn long_names_make_short_ids_that_do_not_end_in_a_dash() {
        let id = id_from_name(&format!("{} tail", "a".repeat(63)));

        assert!(id.len() <= MAX_ID_LEN);
        assert!(!id.ends_with('-'));
        assert!(validate_application_id(&id).is_ok());
    }

    #[test]
    fn a_requested_id_is_checked_and_a_missing_one_comes_from_the_name() {
        assert_eq!(
            resolve_id(Some("shop-web"), "Anything").unwrap(),
            "shop-web"
        );
        assert_eq!(resolve_id(None, "Shop Web").unwrap(), "shop-web");
        assert_eq!(resolve_id(Some("  "), "Shop Web").unwrap(), "shop-web");
        assert!(resolve_id(Some("../x"), "Shop").is_err());
        assert!(resolve_id(Some(&"a".repeat(65)), "Shop").is_err());
    }

    #[test]
    fn a_change_of_segments_is_checked_and_each_is_named_by_an_id() {
        let change: RumApplicationSettingsPatch = serde_json::from_value(serde_json::json!({
            "segments": [{
                "name": "Big customers",
                "conditions": [{ "field": "usr.extra.plan", "op": "is", "values": ["enterprise"] }],
            }],
        }))
        .unwrap();

        assert!(change.validate().is_ok());

        let applied = change.applied_to(&RumApplicationSettings::default());

        assert_eq!(applied.segments[0].id, "big-customers");

        let refused: RumApplicationSettingsPatch = serde_json::from_value(serde_json::json!({
            "segments": [{
                "name": "x",
                "conditions": [{ "field": "view.url_path", "op": "is", "values": ["/a"] }],
            }],
        }))
        .unwrap();

        assert!(refused.validate().is_err());

        let kept = RumApplicationSettings {
            segments: applied.segments.clone(),
            ..RumApplicationSettings::default()
        };

        assert_eq!(
            RumApplicationSettingsPatch::default()
                .applied_to(&kept)
                .segments,
            applied.segments
        );
    }

    #[test]
    fn settings_default_to_collecting_everything() {
        let settings = RumApplicationSettings::default();

        assert!(settings.collect_ip && settings.collect_geo);
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn a_patch_is_checked() {
        let bad_privacy = RumApplicationPatch {
            settings: Some(RumApplicationSettingsPatch {
                default_privacy: Some("none".into()),
                ..RumApplicationSettingsPatch::default()
            }),
            ..RumApplicationPatch::default()
        };
        let empty_name = RumApplicationPatch {
            name: Some("   ".into()),
            ..RumApplicationPatch::default()
        };

        assert!(bad_privacy.validate().is_err());
        assert!(empty_name.validate().is_err());
        assert!(RumApplicationPatch::default().validate().is_ok());
    }

    #[test]
    fn platforms_serialize_as_sources() {
        assert_eq!(
            serde_json::to_string(&RumPlatform::ReactNative).unwrap(),
            "\"react-native\""
        );
        assert_eq!(
            serde_json::from_str::<RumPlatform>("\"flutter\"").unwrap(),
            RumPlatform::Flutter
        );
    }
}
