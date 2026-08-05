// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Per-user preferences persisted as explicit columns.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

use super::sqlx_err;
use crate::shared::{Result, ids::Id, time::TimestampMicros};

fn default_theme() -> String {
    "system".into()
}

fn default_density() -> String {
    "normal".into()
}

fn default_language() -> String {
    "en-us".into()
}

fn default_home_route() -> String {
    "/home".into()
}

fn default_time_format() -> String {
    "iso_24h".into()
}

fn default_date_format() -> String {
    "yyyy_mm_dd_dash".into()
}

/// 空串表示「跟随浏览器时区」——后端取不到浏览器时区，由前端解析为本地时区。
fn default_timezone() -> String {
    String::new()
}

fn default_keyboard_shortcuts_enabled() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserPreferences {
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_density")]
    pub density: String,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_home_route")]
    pub default_home_route: String,
    #[serde(default = "default_time_format")]
    pub time_format: String,
    #[serde(default = "default_date_format")]
    pub date_format: String,
    #[serde(default = "default_timezone")]
    pub timezone: String,
    #[serde(default = "default_keyboard_shortcuts_enabled")]
    pub keyboard_shortcuts_enabled: bool,
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            density: default_density(),
            language: default_language(),
            default_home_route: default_home_route(),
            time_format: default_time_format(),
            date_format: default_date_format(),
            timezone: default_timezone(),
            keyboard_shortcuts_enabled: default_keyboard_shortcuts_enabled(),
        }
    }
}

#[async_trait]
pub trait UserPreferencesRepository: Send + Sync {
    async fn get_optional(&self, user_id: &Id) -> Result<Option<UserPreferences>>;
    async fn get(&self, user_id: &Id) -> Result<UserPreferences> {
        Ok(self.get_optional(user_id).await?.unwrap_or_default())
    }
    async fn upsert(&self, user_id: &Id, preferences: UserPreferences) -> Result<UserPreferences>;
}

pub struct PgUserPreferencesRepository {
    pool: PgPool,
}

impl PgUserPreferencesRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

pub(crate) fn row_to_preferences(row: sqlx::postgres::PgRow) -> Result<UserPreferences> {
    Ok(UserPreferences {
        theme: row.try_get("theme").map_err(sqlx_err)?,
        density: row.try_get("density").map_err(sqlx_err)?,
        language: row.try_get("language").map_err(sqlx_err)?,
        default_home_route: row.try_get("default_home_route").map_err(sqlx_err)?,
        time_format: row.try_get("time_format").map_err(sqlx_err)?,
        date_format: row.try_get("date_format").map_err(sqlx_err)?,
        timezone: row.try_get("timezone").map_err(sqlx_err)?,
        keyboard_shortcuts_enabled: row
            .try_get("keyboard_shortcuts_enabled")
            .map_err(sqlx_err)?,
    })
}

#[async_trait]
impl UserPreferencesRepository for PgUserPreferencesRepository {
    async fn get_optional(&self, user_id: &Id) -> Result<Option<UserPreferences>> {
        let row = sqlx::query(
            "SELECT theme, density, language, default_home_route, time_format,
                    date_format, timezone, keyboard_shortcuts_enabled
             FROM user_preferences
             WHERE user_id = $1",
        )
        .bind(&user_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;
        row.map(row_to_preferences).transpose()
    }

    async fn upsert(&self, user_id: &Id, preferences: UserPreferences) -> Result<UserPreferences> {
        sqlx::query(
            "INSERT INTO user_preferences (
                user_id,
                theme,
                density,
                language,
                default_home_route,
                time_format,
                date_format,
                timezone,
                keyboard_shortcuts_enabled,
                updated_at_micros
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (user_id) DO UPDATE
                SET theme = EXCLUDED.theme,
                    density = EXCLUDED.density,
                    language = EXCLUDED.language,
                    default_home_route = EXCLUDED.default_home_route,
                    time_format = EXCLUDED.time_format,
                    date_format = EXCLUDED.date_format,
                    timezone = EXCLUDED.timezone,
                    keyboard_shortcuts_enabled = EXCLUDED.keyboard_shortcuts_enabled,
                    updated_at_micros = EXCLUDED.updated_at_micros",
        )
        .bind(&user_id.0)
        .bind(&preferences.theme)
        .bind(&preferences.density)
        .bind(&preferences.language)
        .bind(&preferences.default_home_route)
        .bind(&preferences.time_format)
        .bind(&preferences.date_format)
        .bind(&preferences.timezone)
        .bind(preferences.keyboard_shortcuts_enabled)
        .bind(TimestampMicros::now().0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_defaults_are_complete() {
        let p = UserPreferences::default();
        assert_eq!(p.theme, "system");
        assert_eq!(p.density, "normal");
        assert_eq!(p.language, "en-us");
        assert_eq!(p.default_home_route, "/home");
        assert_eq!(p.time_format, "iso_24h");
        assert_eq!(p.date_format, "yyyy_mm_dd_dash");
        assert_eq!(p.timezone, "");
        assert!(p.keyboard_shortcuts_enabled);
    }
}
