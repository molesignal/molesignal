// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 打开分享链接：公开链接换一个短时的分享会话，其他范围跳到资源本身。
//!
//! 打不开的链接跳到「链接不可用」页面：拿着链接的人在浏览器里，后端的回答他们看不懂，也没有别的路可走。

use axum::{
    body::Body,
    extract::{Path, State},
    http::{
        StatusCode,
        header::{CACHE_CONTROL, LOCATION, REFERRER_POLICY, SET_COOKIE},
    },
    response::{IntoResponse, Response},
};
use serde_json::json;

use super::{
    SHARE_SESSION_COOKIE,
    audit::{ShareVisitor, record_audit},
    share_is_active, token_hash,
};
use crate::{
    api::AppState,
    infra::persistence::repositories::{
        api_tokens::generate_token,
        resource_shares::{ResourceShare, ResourceShareMode, ResourceShareSession},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const SHARE_SESSION_SECS: i64 = 30 * 60;
/// `web/src/routes/PublicShare.tsx` 认这个标记。公开访问的代理配置里 `/shared` 是精确匹配，所以用查询
/// 参数，不另开路径。
const UNAVAILABLE_PAGE: &str = "/shared?unavailable=1";

pub(super) async fn resolve_share_link(
    State(state): State<AppState>,
    Path(token): Path<String>,
    visitor: ShareVisitor,
) -> Result<Response> {
    let now = TimestampMicros::now();

    let Some(share) = state
        .storage
        .resource_shares
        .find_by_token_hash(&token_hash(&token))
        .await?
    else {
        return Ok(redirect_to(UNAVAILABLE_PAGE));
    };

    let organization = state
        .iam
        .service
        .ensure_organization_access(&share.organization_id)
        .await;

    // 组织被删除或停用，它的链接也就打不开了。
    if let Err(Error::Unauthorized(_) | Error::Forbidden(_)) = organization {
        return Ok(redirect_to(UNAVAILABLE_PAGE));
    }

    organization?;

    if !share_is_active(&share, now) {
        return Ok(redirect_to(UNAVAILABLE_PAGE));
    }

    if share.share_mode != ResourceShareMode::PublicLink {
        let location = authenticated_target(&share)?;

        record_audit(
            &state,
            &share,
            "resource_share.open",
            "share_link",
            &share.id.0,
            Some(&visitor),
            json!({"result": "redirect_to_authenticated_resource"}),
        )
        .await?;

        return Ok(redirect_to(&location));
    }

    let raw_session = generate_token();

    let session = ResourceShareSession {
        id: Id::new(),
        share_id: share.id.clone(),
        session_token_hash: token_hash(&raw_session),
        password_verified: share.password_hash.is_none(),
        created_at: now,
        expires_at: TimestampMicros(
            now.0
                .saturating_add(SHARE_SESSION_SECS.saturating_mul(1_000_000)),
        ),
        last_seen_at: now,
        ip: visitor.ip.clone(),
        user_agent: visitor.user_agent.clone(),
    };

    let share = match state
        .storage
        .resource_shares
        .create_session(session, now)
        .await
    {
        Ok(share) => share,
        // 访问次数用完了，或者刚被停用。
        Err(Error::NotFound(_)) => return Ok(redirect_to(UNAVAILABLE_PAGE)),
        Err(error) => return Err(error),
    };

    let mut cookie = format!(
        "{SHARE_SESSION_COOKIE}={raw_session}; Path=/; Max-Age={SHARE_SESSION_SECS}; HttpOnly; SameSite=Lax"
    );

    if state.platform.external_url.trim().starts_with("https://") {
        cookie.push_str("; Secure");
    }

    record_audit(
        &state,
        &share,
        "resource_share.open",
        "share_session",
        &share.id.0,
        Some(&visitor),
        json!({"result": "session_created", "viewCount": share.view_count}),
    )
    .await?;

    Response::builder()
        .status(StatusCode::FOUND)
        .header(LOCATION, "/shared")
        .header(SET_COOKIE, cookie)
        .header(CACHE_CONTROL, "no-store")
        .header(REFERRER_POLICY, "no-referrer")
        .body(Body::empty())
        .map_err(|error| Error::internal(format!("share redirect response: {error}")))
}

/// 令牌在地址里，所以不留缓存，也不带进下一页的 Referer。
fn redirect_to(location: &str) -> Response {
    (
        StatusCode::FOUND,
        [
            (LOCATION, location),
            (CACHE_CONTROL, "no-store"),
            (REFERRER_POLICY, "no-referrer"),
        ],
    )
        .into_response()
}

fn authenticated_target(share: &ResourceShare) -> Result<String> {
    match share.resource_type.as_str() {
        "dashboard" => Ok(format!("/dashboards/{}", share.resource_id.0)),
        "report" | "report_file" => Ok(format!("/reports?report={}", share.resource_id.0)),
        _ => Err(Error::invalid("unsupported shared resource type")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_that_cannot_open_lands_on_the_page_and_leaves_its_token_behind() {
        let response = redirect_to(UNAVAILABLE_PAGE);

        assert_eq!(response.status(), StatusCode::FOUND);
        assert_eq!(response.headers()[LOCATION], "/shared?unavailable=1");
        assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
        assert_eq!(response.headers()[REFERRER_POLICY], "no-referrer");
    }
}
