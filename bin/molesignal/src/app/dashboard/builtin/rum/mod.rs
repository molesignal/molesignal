// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The two dashboards every organization with RUM applications gets: one for the applications that
//! run in a browser and one for those that run on a phone. Both are SQL on the `rum_*` streams,
//! read-only, and meant to be copied before they are changed.

mod browser;
mod mobile;
mod sql;

use serde_json::{Value, json};

use self::sql::{EVERY_ENVIRONMENT, platform};
use crate::{domain::rum::RumPlatform, shared::ids::Id};

/// The version of the content of the dashboards this build ships. Raise it with every change of a
/// panel, a variable or a title: an organization whose dashboard is older gets the new one.
pub const VERSION: u32 = 3;

/// The author the dashboards the product installs are written by.
pub(in crate::app::dashboard) const AUTHOR: &str = "molesignal-system";

/// When the content of the current version was written; the models carry it so that an
/// installation of the same version is byte for byte the same.
const WRITTEN: &str = "2026-10-25T00:00:00Z";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RumDashboard {
    Browser,
    Mobile,
}

impl RumDashboard {
    pub const ALL: [Self; 2] = [Self::Browser, Self::Mobile];

    /// The dashboard that shows applications of a platform; none for a platform that has no
    /// views of its own.
    pub fn of(platform: RumPlatform) -> Option<Self> {
        match platform {
            RumPlatform::Browser => Some(Self::Browser),
            RumPlatform::Android
            | RumPlatform::Ios
            | RumPlatform::Flutter
            | RumPlatform::ReactNative => Some(Self::Mobile),
            RumPlatform::Other => None,
        }
    }

    /// What the dashboard is called to the organization: fixed, so the same one is found again.
    pub const fn uid(self) -> &'static str {
        match self {
            Self::Browser => "rum-browser",
            Self::Mobile => "rum-mobile",
        }
    }

    /// The id the dashboard has in an organization: the uid and the organization, so that the
    /// pages can link to it without asking for it, and two organizations never share one.
    pub fn id(self, org_id: &Id) -> Id {
        Id::from_string(format!("{}-{}", self.uid(), org_id.0))
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Browser => "RUM · 浏览器应用",
            Self::Mobile => "RUM · 移动应用",
        }
    }

    pub const fn tags(self) -> [&'static str; 2] {
        ["内置", "rum"]
    }

    const fn description(self) -> &'static str {
        match self {
            Self::Browser => {
                "浏览器应用的会话、Web Vitals、错误、页面与版本。数字与 RUM 概览同一个算法：会话取最新版本，p75 只算有值的视图。"
            }
            Self::Mobile => {
                "移动应用的会话、崩溃、错误、屏幕与版本。数字与 RUM 概览同一个算法：会话取最新版本。"
            }
        }
    }

    /// The `source` the events of the platform carry.
    fn sources(self) -> &'static [&'static str] {
        match self {
            Self::Browser => &["browser"],
            Self::Mobile => &["android", "ios", "flutter", "react-native"],
        }
    }

    /// The model of the dashboard, with the identity the organization stores it under.
    pub fn model(self, id: &Id) -> Value {
        let panels = match self {
            Self::Browser => browser::panels(),
            Self::Mobile => mobile::panels(),
        };

        json!({
            "engine": "molesignal-dashboard",
            "schemaVersion": 2,
            "id": id.0,
            "uid": self.uid(),
            "title": self.title(),
            "description": self.description(),
            "tags": self.tags(),
            "editable": false,
            "defaultDashboard": false,
            "timeSettings": {
                "defaultFrom": "now-24h",
                "defaultTo": "now",
                "timezone": "browser",
            },
            "refreshSettings": {
                "enabled": true,
                "mode": "interval",
                "defaultInterval": "1m",
                "allowedIntervals": ["off", "1m", "5m"],
            },
            "interactionSettings": { "cursorSync": "shared_crosshair" },
            "variables": self.variables(),
            "annotations": [],
            "links": [],
            "layout": { "type": "grid", "columns": 24, "rowHeight": 8, "gap": 8 },
            "elements": panels,
            "version": 1,
            "createdAt": WRITTEN,
            "updatedAt": WRITTEN,
            "createdBy": AUTHOR,
            "updatedBy": AUTHOR,
            "extensions": { "builtin": { "key": self.uid(), "version": VERSION } },
        })
    }

    /// The application, its environment, and the width of the bars of a chart over time. The
    /// application and the environment are chosen among those with data; they travel in the
    /// address of the dashboard as `var-application` and `var-environment`.
    fn variables(self) -> Value {
        let applications = format!(
            r#"SELECT DISTINCT "application.id" FROM rum_sessions WHERE {} ORDER BY 1"#,
            platform(self.sources())
        );

        let environments = format!(
            r#"SELECT DISTINCT env FROM rum_sessions WHERE "application.id" = ${{application:sqlstring}} AND env <> '' UNION ALL SELECT '{EVERY_ENVIRONMENT}'"#
        );

        json!([
            {
                "id": "variable-application",
                "name": "application",
                "label": "应用",
                "type": "query",
                "query": {
                    "kind": "sql",
                    "streamName": "rum_sessions",
                    "streamType": "logs",
                    "expression": applications,
                },
                "multi": false,
                "includeAll": false,
                "hide": "none",
                "refresh": "time_range_change",
            },
            {
                "id": "variable-environment",
                "name": "environment",
                "label": "环境",
                "type": "query",
                "query": {
                    "kind": "sql",
                    "streamName": "rum_sessions",
                    "streamType": "logs",
                    "expression": environments,
                },
                "options": [
                    { "label": EVERY_ENVIRONMENT, "value": EVERY_ENVIRONMENT, "selected": true },
                ],
                "defaultValue": EVERY_ENVIRONMENT,
                "currentValue": EVERY_ENVIRONMENT,
                "multi": false,
                "includeAll": false,
                "hide": "none",
                "refresh": "time_range_change",
                "dependsOn": ["application"],
            },
            {
                "id": "variable-interval",
                "name": "interval",
                "label": "间隔",
                "type": "interval",
                "options": [
                    { "label": "5 分钟", "value": "5 minutes" },
                    { "label": "15 分钟", "value": "15 minutes", "selected": true },
                    { "label": "1 小时", "value": "1 hour" },
                ],
                "defaultValue": "15 minutes",
                "currentValue": "15 minutes",
                "multi": false,
                "includeAll": false,
                "hide": "none",
                "refresh": "never",
            },
        ])
    }
}

/// The version of the content a stored model of a built-in dashboard holds; zero for a model that
/// is not one.
pub fn installed_version(model: &Value) -> u32 {
    model
        .pointer("/extensions/builtin/version")
        .and_then(Value::as_u64)
        .and_then(|version| u32::try_from(version).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
