// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! IM 里写给人看的话：`i18n_language` 的 `agent.im.*`，用提问人的界面语言；还对不上人时用
//! 工作区的默认语言。

use std::sync::Arc;

use crate::{
    agent::im::ImPlatform,
    api::AppState,
    domain::i18n_language::{FALLBACK_LANGUAGE_CODE, I18nLanguageRepository, LanguagePack},
    infra::persistence::repositories::{
        user_preferences::UserPreferencesRepository,
        workspace_preference_defaults::WorkspacePreferenceDefaultsRepository,
    },
    shared::ids::Id,
};

pub(super) struct Texts {
    pack: LanguagePack,
    pub(super) language: String,
}

/// 读 IM 文案要的东西：人的界面语言、工作区的默认语言与语言表。
#[derive(Clone)]
pub(crate) struct TextSources {
    pub(crate) user_preferences: Arc<dyn UserPreferencesRepository>,
    pub(crate) workspace_defaults: Arc<dyn WorkspacePreferenceDefaultsRepository>,
    pub(crate) i18n_language: Arc<dyn I18nLanguageRepository>,
}

impl Texts {
    /// 一个人的界面语言；没设时用工作区的默认。
    pub(super) async fn for_user(state: &AppState, org_id: &Id, user_id: Option<&Id>) -> Self {
        Self::load(
            state.iam.user_preferences.as_ref(),
            state.iam.workspace_preference_defaults.as_ref(),
            state.platform.i18n_language.as_ref(),
            org_id,
            user_id,
        )
        .await
    }

    /// 同 [`Self::for_user`]，只用到 `sources`。
    pub(super) async fn from(sources: &TextSources, org_id: &Id, user_id: Option<&Id>) -> Self {
        Self::load(
            sources.user_preferences.as_ref(),
            sources.workspace_defaults.as_ref(),
            sources.i18n_language.as_ref(),
            org_id,
            user_id,
        )
        .await
    }

    async fn load(
        user_preferences: &dyn UserPreferencesRepository,
        workspace_defaults: &dyn WorkspacePreferenceDefaultsRepository,
        i18n_language: &dyn I18nLanguageRepository,
        org_id: &Id,
        user_id: Option<&Id>,
    ) -> Self {
        let personal = match user_id {
            Some(user_id) => user_preferences
                .get_optional(user_id)
                .await
                .ok()
                .flatten()
                .map(|preference| preference.language),
            None => None,
        };

        let language = match personal {
            Some(language) => language,
            None => workspace_defaults
                .get(org_id)
                .await
                .map(|defaults| defaults.language)
                .unwrap_or_else(|_| FALLBACK_LANGUAGE_CODE.to_string()),
        };

        let pack = i18n_language
            .load_pack("agent.im.", &language)
            .await
            .unwrap_or_else(|error| {
                tracing::warn!(%error, "the IM texts could not be read");
                LanguagePack::default()
            });

        Self { pack, language }
    }

    /// 没有文案：写出键名。
    #[cfg(test)]
    pub(super) fn empty() -> Self {
        Self {
            pack: LanguagePack::default(),
            language: FALLBACK_LANGUAGE_CODE.to_string(),
        }
    }

    /// 句子里的平台名（中文里 Slack 两边带空格）。
    pub(super) fn platform(&self, platform: ImPlatform) -> String {
        self.pack
            .get(&format!("agent.im.platform.{}", platform.as_str()))
            .map_or_else(|| platform.label().to_string(), str::to_string)
    }

    /// `agent.im.<key>`，填上 `fills`；缺了这一条时写出键名，不让回复落空。
    pub(super) fn get(&self, key: &str, fills: &[(&str, &str)]) -> String {
        let key = format!("agent.im.{key}");

        self.pack.render(&key, fills).unwrap_or(key)
    }
}
