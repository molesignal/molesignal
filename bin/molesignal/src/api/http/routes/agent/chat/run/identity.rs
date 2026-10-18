// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Run 以谁的身份执行：发起这一轮的人。执行时按 Run 记下的身份重新核对，与请求进来时的鉴权是
//! 同一套：账号与组织还能用，再解析成员关系与权限。排队这段时间里被收回的权限不会被用上。

use crate::{
    agent::runs::Run,
    api::AppState,
    app::iam::IamContext,
    shared::{Error, Result},
};

pub(super) async fn resolve(state: &AppState, run: &Run) -> Result<IamContext> {
    let user_id = run.initiator();

    let admitted = async {
        state.iam.service.ensure_user_access(user_id).await?;
        state
            .iam
            .service
            .ensure_organization_access(&run.org_id)
            .await
    };

    admitted.await.map_err(unusable)?;

    let mut context = run.principal.unresolved_context(&run.org_id);
    state
        .iam
        .access
        .enrich_context(&mut context)
        .await
        .map_err(unusable)?;

    if !context.has_permission("agent.use") {
        return Err(Error::forbidden(
            "the person who asked no longer holds agent.use",
        ));
    }

    Ok(context)
}

fn unusable(error: Error) -> Error {
    match error {
        Error::Unauthorized(reason) | Error::Forbidden(reason) => Error::forbidden(format!(
            "the person who asked can no longer use this organization ({reason})"
        )),
        other => other,
    }
}
