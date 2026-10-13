// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Text the backend writes for the person who is signed in, in the language
//! they read the interface in.
//!
//! The text is in the `i18n_language` table. What is read through here names
//! things the product ships with, a built-in prompt or a built-in function,
//! and each of those also keeps a text of its own in its own table. A caller
//! shows that one when the entry is missing.

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::i18n_language::{FALLBACK_LANGUAGE_CODE, LanguagePack},
    shared::Result,
};

/// The language code the signed-in user reads the interface in: their own
/// preference, or the default of the workspace while they have set none.
pub(crate) async fn interface_language(state: &AppState, ctx: &IamContext) -> Result<String> {
    if let Some(personal) = state
        .iam
        .user_preferences
        .get_optional(&ctx.user_id)
        .await?
    {
        return Ok(personal.language);
    }

    let workspace = state
        .iam
        .workspace_preference_defaults
        .get(&ctx.org_id)
        .await?;

    Ok(workspace.language)
}

/// The entries under `key_prefix` in that language.
///
/// A pack that cannot be read is an empty one, and the log says so: a list
/// is not refused over the names of what it lists.
pub(crate) async fn interface_language_pack(
    state: &AppState,
    ctx: &IamContext,
    key_prefix: &str,
) -> LanguagePack {
    let language_code = match interface_language(state, ctx).await {
        Ok(language_code) => language_code,
        Err(error) => {
            tracing::warn!(error = %error, "the interface language could not be read");
            FALLBACK_LANGUAGE_CODE.to_string()
        }
    };

    match state
        .platform
        .i18n_language
        .load_pack(key_prefix, &language_code)
        .await
    {
        Ok(pack) => pack,
        Err(error) => {
            tracing::warn!(error = %error, key_prefix, "language entries could not be read");
            LanguagePack::default()
        }
    }
}
