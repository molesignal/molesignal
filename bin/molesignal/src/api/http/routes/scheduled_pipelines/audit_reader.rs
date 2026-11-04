// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Who a pipeline reads the audit trail as, by name. It comes from the user record: whoever may read
//! a pipeline sees whose access it uses, whether or not they may list the organization's members.

use super::Resp;
use crate::{api::AppState, app::reporting::run_as_profiles, shared::ids::Id};

/// Fills in who these pipelines read the audit trail as. They come back with the id alone when the
/// user table cannot be read, and without a name when the user is gone.
pub(super) async fn name_readers(state: &AppState, pipelines: Vec<&mut Resp>) {
    let ids: Vec<Id> = pipelines
        .iter()
        .filter_map(|pipeline| pipeline.audit_reader_id.clone().map(Id))
        .collect();

    let profiles = match run_as_profiles(state.iam.service.users.as_ref(), &ids).await {
        Ok(profiles) => profiles,
        Err(error) => {
            tracing::warn!(%error, "the people pipelines read the audit trail as could not be read");
            return;
        }
    };

    for pipeline in pipelines {
        pipeline.audit_reader = pipeline
            .audit_reader_id
            .clone()
            .and_then(|id| profiles.get(&Id(id)).cloned());
    }
}

/// [`name_readers`] for one pipeline.
pub(super) async fn named(state: &AppState, mut pipeline: Resp) -> Resp {
    name_readers(state, vec![&mut pipeline]).await;
    pipeline
}
