// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 每个用户的界面偏好，存在 `users_ext.preferences`；`preferences_updated_at` 为空表示本人还没保存过。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row, types::Json};

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
#[serde(rename_all = "camelCase")]
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

#[async_trait]
impl UserPreferencesRepository for PgUserPreferencesRepository {
    async fn get_optional(&self, user_id: &Id) -> Result<Option<UserPreferences>> {
        let row = sqlx::query(
            "SELECT preferences FROM users_ext
             WHERE user_id = $1 AND preferences_updated_at IS NOT NULL",
        )
        .bind(&user_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        row.map(|row| {
            row.try_get::<Json<UserPreferences>, _>("preferences")
                .map(|preferences| preferences.0)
                .map_err(sqlx_err)
        })
        .transpose()
    }

    async fn upsert(&self, user_id: &Id, preferences: UserPreferences) -> Result<UserPreferences> {
        sqlx::query(
            "INSERT INTO users_ext (user_id, preferences, preferences_updated_at)
             VALUES ($1, $2, $3)
             ON CONFLICT (user_id) DO UPDATE
                SET preferences = EXCLUDED.preferences,
                    preferences_updated_at = EXCLUDED.preferences_updated_at",
        )
        .bind(&user_id.0)
        .bind(Json(&preferences))
        .bind(TimestampMicros::now())
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
