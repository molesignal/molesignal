// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 浏览器登录会话的 Cookie。
//!
//! 登录、刷新和切换组织时，服务端把 access token 与 refresh token 写进两个 HttpOnly Cookie，
//! 页面脚本读不到它们。access token 的 Cookie 只发往 `/api`，refresh token 的只发往换新
//! access token 的 `/api/v1/auth`。两个都是 `SameSite=Strict`，别的站点发起的请求不带；写操作
//! 还要浏览器标明请求来自本站页面（`Sec-Fetch-Site: same-origin`）。
//!
//! 同一浏览器的标签页共用这对 Cookie：一个标签页切换组织或换人登录后，其它标签页的请求也
//! 属于新的会话。页面因此在每个请求里用 [`SESSION_HEADER`] 说明自己显示的是哪位用户、哪个
//! 组织；与 Cookie 不符时服务端拒绝请求（409 `session_changed`），由页面重新加载。

use axum::{
    Json,
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{COOKIE, SET_COOKIE},
    },
    response::{IntoResponse, IntoResponseParts, Response, ResponseParts},
};

use crate::{
    api::AppState,
    app::iam::{SignInMethod, SignInSession},
    domain::iam::IamScope,
    shared::{Error, Result, SessionCookie, ids::Id},
};

pub(crate) const ACCESS_COOKIE: &str = "accessToken";
const ACCESS_COOKIE_PATH: &str = "/api";
pub(crate) const REFRESH_COOKIE: &str = "refreshToken";
const REFRESH_COOKIE_PATH: &str = "/api/v1/auth";

/// 页面显示的会话：`<user_id>:<org_id>`。
pub(crate) const SESSION_HEADER: &str = "x-molesignal-session";

/// 写进响应的 `Set-Cookie`。
pub(crate) struct SessionCookies(Vec<HeaderValue>);

impl IntoResponseParts for SessionCookies {
    type Error = std::convert::Infallible;

    fn into_response_parts(
        self,
        mut response: ResponseParts,
    ) -> std::result::Result<ResponseParts, Self::Error> {
        for cookie in self.0 {
            response.headers_mut().append(SET_COOKIE, cookie);
        }

        Ok(response)
    }
}

/// 开始一次登录：签发两个 token，写进两个 Cookie。
pub(crate) fn sign_in(
    state: &AppState,
    headers: &HeaderMap,
    (user_id, org_id, scope): (&Id, &Id, IamScope),
    remember: bool,
    method: SignInMethod,
) -> Result<SessionCookies> {
    let session = state
        .iam
        .service
        .begin_session(user_id, org_id, scope, remember, method);

    issue(state, headers, &session)
}

/// 为 `session` 签发 access token 与 refresh token。
pub(crate) fn issue(
    state: &AppState,
    headers: &HeaderMap,
    session: &SignInSession,
) -> Result<SessionCookies> {
    ensure_same_origin(headers)?;
    let tokens = state.iam.service.issue_session_tokens(session)?;
    let writer = CookieWriter::new(state, headers);

    Ok(SessionCookies(vec![
        writer.set(
            ACCESS_COOKIE,
            ACCESS_COOKIE_PATH,
            &tokens.access_token,
            session.remember.then_some(tokens.access_expires_at),
        )?,
        writer.set(
            REFRESH_COOKIE,
            REFRESH_COOKIE_PATH,
            &tokens.refresh_token,
            session.remember.then_some(session.expires_at),
        )?,
    ]))
}

/// 同一次登录换一个新的 access token。
pub(crate) fn renew(
    state: &AppState,
    headers: &HeaderMap,
    session: &SignInSession,
) -> Result<SessionCookies> {
    ensure_same_origin(headers)?;
    let (token, expires_at) = state.iam.service.issue_access_token(session)?;

    Ok(SessionCookies(vec![
        CookieWriter::new(state, headers).set(
            ACCESS_COOKIE,
            ACCESS_COOKIE_PATH,
            &token,
            session.remember.then_some(expires_at),
        )?,
    ]))
}

/// 让浏览器删掉两个 Cookie。
pub(crate) fn clear(state: &AppState, headers: &HeaderMap) -> SessionCookies {
    let writer = CookieWriter::new(state, headers);

    SessionCookies(
        [
            (ACCESS_COOKIE, ACCESS_COOKIE_PATH),
            (REFRESH_COOKIE, REFRESH_COOKIE_PATH),
        ]
        .into_iter()
        .filter_map(|(name, path)| writer.expire(name, path))
        .collect(),
    )
}

/// 请求带的 access token。
pub(crate) fn access_token(headers: &HeaderMap) -> Option<&str> {
    cookie(headers, ACCESS_COOKIE)
}

/// 请求带的 refresh token。
pub(crate) fn refresh_token(headers: &HeaderMap) -> Option<&str> {
    cookie(headers, REFRESH_COOKIE)
}

/// 无头浏览器渲染报表时带的 access token Cookie。
pub(crate) fn render_cookie(access_token: String) -> SessionCookie {
    SessionCookie {
        name: ACCESS_COOKIE.to_owned(),
        value: access_token,
        path: ACCESS_COOKIE_PATH.to_owned(),
    }
}

/// 改动数据的请求只接受本站页面发起的。浏览器在 `Sec-Fetch-Site` 里标明请求从哪里来；
/// 不带这个请求头的不是浏览器，别的网站借不到它的 Cookie。
pub(crate) fn ensure_same_origin(headers: &HeaderMap) -> Result<()> {
    match headers.get("sec-fetch-site").map(HeaderValue::as_bytes) {
        None | Some(b"same-origin") => Ok(()),
        Some(_) => Err(Error::forbidden(
            "the session cookie is only accepted from MoleSignal's own pages",
        )),
    }
}

/// 页面在 [`SESSION_HEADER`] 里说明它显示的会话；与 Cookie 所属的不同，说明另一个标签页
/// 换了组织或换了人登录。这时返回拒绝请求的应答。
pub(crate) fn refuse_other_session(
    headers: &HeaderMap,
    session: &SignInSession,
) -> Option<Response> {
    let displayed = headers.get(SESSION_HEADER)?;
    let current = format!("{}:{}", session.user_id.0, session.org_id.0);

    if displayed.as_bytes() == current.as_bytes() {
        return None;
    }

    Some(
        (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "session_changed",
                "message": "this browser is now signed in to another organization or account; reload the page",
            })),
        )
            .into_response(),
    )
}

fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(key, value)| *key == name && !value.is_empty())
        .map(|(_, value)| value)
}

/// 同一个响应里的 Cookie 共用的属性。
struct CookieWriter {
    secure: bool,
    now: i64,
}

impl CookieWriter {
    fn new(state: &AppState, headers: &HeaderMap) -> Self {
        Self {
            secure: served_over_https(state, headers),
            now: chrono::Utc::now().timestamp(),
        }
    }

    /// `expires_at` 为空时是会话 Cookie，关掉浏览器就没了。
    fn set(
        &self,
        name: &str,
        path: &str,
        value: &str,
        expires_at: Option<i64>,
    ) -> Result<HeaderValue> {
        let mut cookie = self.attributes(name, path, value);

        if let Some(expires_at) = expires_at {
            cookie.push_str(&format!("; Max-Age={}", (expires_at - self.now).max(0)));
        }

        HeaderValue::from_str(&cookie)
            .map_err(|_| Error::internal("session cookie is not a valid header value"))
    }

    fn expire(&self, name: &str, path: &str) -> Option<HeaderValue> {
        HeaderValue::from_str(&format!("{}; Max-Age=0", self.attributes(name, path, ""))).ok()
    }

    fn attributes(&self, name: &str, path: &str, value: &str) -> String {
        let secure = if self.secure { "; Secure" } else { "" };

        format!("{name}={value}; Path={path}; HttpOnly; SameSite=Strict{secure}")
    }
}

/// 浏览器是否经 HTTPS 访问。请求的 `Origin` 最准；没有时看反向代理的
/// `X-Forwarded-Proto` 和配置的对外地址。
fn served_over_https(state: &AppState, headers: &HeaderMap) -> bool {
    let header = |name: &str| headers.get(name).and_then(|value| value.to_str().ok());

    match header("origin") {
        Some(origin) if origin.starts_with("https://") => true,
        Some(origin) if origin.starts_with("http://") => false,
        _ => {
            header("x-forwarded-proto").is_some_and(|proto| proto.eq_ignore_ascii_case("https"))
                || state.platform.external_url.trim().starts_with("https://")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();

        for (name, value) in pairs {
            headers.append(*name, value.parse().unwrap());
        }

        headers
    }

    fn session(user: &str, org: &str) -> SignInSession {
        SignInSession {
            user_id: Id::from_string(user),
            org_id: Id::from_string(org),
            scope: IamScope::Organization,
            expires_at: 0,
            remember: false,
            method: SignInMethod::single_factor(),
            auth_time: crate::shared::time::TimestampMicros(0),
            verified_at: crate::shared::time::TimestampMicros(0),
        }
    }

    #[test]
    fn tokens_are_read_from_their_own_cookie() {
        let request = headers(&[
            ("cookie", "theme=dark; accessToken=a.b.c"),
            ("cookie", "refreshToken=r.s.t"),
        ]);

        assert_eq!(access_token(&request), Some("a.b.c"));
        assert_eq!(refresh_token(&request), Some("r.s.t"));
        assert_eq!(access_token(&headers(&[("cookie", "accessToken=")])), None);
        assert_eq!(
            access_token(&headers(&[("cookie", "xaccessToken=a")])),
            None
        );
    }

    #[test]
    fn writes_need_a_page_of_this_site() {
        assert!(ensure_same_origin(&headers(&[])).is_ok());
        assert!(ensure_same_origin(&headers(&[("sec-fetch-site", "same-origin")])).is_ok());

        for site in ["same-site", "cross-site", "none"] {
            assert!(matches!(
                ensure_same_origin(&headers(&[("sec-fetch-site", site)])),
                Err(Error::Forbidden(_))
            ));
        }
    }

    #[test]
    fn a_page_showing_another_session_is_told_to_reload() {
        let current = session("u1", "org-a");

        assert!(refuse_other_session(&headers(&[]), &current).is_none());
        assert!(
            refuse_other_session(&headers(&[(SESSION_HEADER, "u1:org-a")]), &current).is_none()
        );

        for displayed in ["u1:org-b", "u2:org-a"] {
            let refused = refuse_other_session(&headers(&[(SESSION_HEADER, displayed)]), &current)
                .expect("another session is refused");
            assert_eq!(refused.status(), StatusCode::CONFLICT);
        }
    }

    #[test]
    fn cookies_are_http_only_strict_and_persist_only_when_remembered() {
        let writer = CookieWriter {
            secure: true,
            now: 1_000,
        };

        assert_eq!(
            writer.set("accessToken", "/api", "t", None).unwrap(),
            "accessToken=t; Path=/api; HttpOnly; SameSite=Strict; Secure"
        );
        assert_eq!(
            writer
                .set("refreshToken", "/api/v1/auth", "r", Some(1_600))
                .unwrap(),
            "refreshToken=r; Path=/api/v1/auth; HttpOnly; SameSite=Strict; Secure; Max-Age=600"
        );
        assert_eq!(
            CookieWriter {
                secure: false,
                now: 0
            }
            .expire("accessToken", "/api")
            .unwrap(),
            "accessToken=; Path=/api; HttpOnly; SameSite=Strict; Max-Age=0"
        );
    }
}
