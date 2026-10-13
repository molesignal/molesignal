// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `i18n_language`: the text the backend writes itself, by key and language code.

use async_trait::async_trait;
use sqlx::PgPool;

use super::sqlx_err;
use crate::{
    domain::i18n_language::{FALLBACK_LANGUAGE_CODE, I18nLanguageRepository, LanguagePack},
    shared::Result,
};

pub struct PgI18nLanguageRepository {
    pool: PgPool,
}

impl PgI18nLanguageRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl I18nLanguageRepository for PgI18nLanguageRepository {
    async fn load_pack(&self, key_prefix: &str, language_code: &str) -> Result<LanguagePack> {
        // One row per key: the requested language when it has the key, the fallback otherwise.
        let entries: Vec<(String, String)> = sqlx::query_as(
            "SELECT DISTINCT ON (language_key) language_key, language_value
             FROM i18n_language
             WHERE starts_with(language_key, $1) AND language_code IN ($2, $3)
             ORDER BY language_key, (language_code = $2) DESC",
        )
        .bind(key_prefix)
        .bind(language_code)
        .bind(FALLBACK_LANGUAGE_CODE)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(LanguagePack::new(entries.into_iter().collect()))
    }
}
