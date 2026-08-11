// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Current authenticated user profile and preferences.

use axum::{
    Extension, Json, Router,
    body::{Body, Bytes},
    extract::{Path, State},
    http::{
        HeaderMap,
        header::{CACHE_CONTROL, CONTENT_TYPE},
    },
    response::Response,
    routing::{get, post},
};
use object_store::{ObjectStoreExt, PutPayload, path::Path as ObjPath};
use serde::{Deserialize, Serialize};

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::permission,
    infra::persistence::repositories::user_preferences::UserPreferences,
    shared::{Error, Result, ids::Id},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/me/profile", get(profile).put(update_profile))
        .route("/me/avatar", post(upload_avatar))
        .route(
            "/me/preferences",
            get(get_preferences).put(update_preferences),
        )
        .route(
            "/workspace/preferences",
            get(get_workspace_preferences).put(update_workspace_preferences),
        )
}

/// 公开端点（auth 白名单，顶层 merge）：`<img src>` 渲染头像时不会带 Bearer，
/// 所以头像读取走免鉴权、内容寻址（路径含随机 id）的公开 GET。
pub fn avatar_serve_routes() -> Router<AppState> {
    Router::new().route("/api/v1/public/avatars/{user_id}/{file}", get(serve_avatar))
}

#[derive(Debug, Serialize)]
struct MeProfileResp {
    user_id: String,
    email: String,
    username: String,
    display_name: String,
    avatar_url: Option<String>,
    bio: String,
    org_id: String,
    org_name: String,
    org_slug: String,
    display_role: String,
    created_at_micros: i64,
}

async fn profile(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<MeProfileResp>> {
    let user = state.iam.service.users.get(&ctx.user_id).await?;
    let org = state.iam.service.orgs.get(&ctx.org_id).await?;
    Ok(Json(MeProfileResp::from_parts(user, org, ctx.display_role)))
}

#[derive(Debug, Deserialize)]
struct UpdateProfileReq {
    display_name: Option<String>,
    username: Option<String>,
    bio: Option<String>,
    // email 是不可变身份字段——即便客户端带上也忽略（serde 容忍未知/多余字段）。
    avatar_url: Option<String>,
}

async fn update_profile(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<UpdateProfileReq>,
) -> Result<Json<MeProfileResp>> {
    let mut user = state.iam.service.users.get(&ctx.user_id).await?;
    if let Some(name) = req.display_name.or(req.username) {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(Error::invalid("display_name must not be empty"));
        }
        if name.chars().count() > 255 {
            return Err(Error::invalid(
                "display_name must be at most 255 characters",
            ));
        }
        user.display_name = name;
    }
    // email 不可修改：忽略请求里的 email。
    if let Some(avatar_url) = req.avatar_url {
        user.avatar_url = normalize_avatar_url(avatar_url)?;
    }
    if let Some(bio) = req.bio {
        let bio = bio.trim().to_string();
        if bio.chars().count() > 500 {
            return Err(Error::invalid("bio must be at most 500 characters"));
        }
        user.bio = bio;
    }
    let user = state.iam.service.users.update(user).await?;
    let org = state.iam.service.orgs.get(&ctx.org_id).await?;
    Ok(Json(MeProfileResp::from_parts(user, org, ctx.display_role)))
}

async fn get_preferences(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<UserPreferences>> {
    if let Some(personal) = state
        .iam
        .user_preferences
        .get_optional(&ctx.user_id)
        .await?
    {
        return Ok(Json(personal));
    }
    Ok(Json(
        state
            .iam
            .workspace_preference_defaults
            .get(&ctx.org_id)
            .await?,
    ))
}

async fn update_preferences(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(mut preferences): Json<UserPreferences>,
) -> Result<Json<UserPreferences>> {
    normalize_and_validate(&mut preferences)?;
    Ok(Json(
        state
            .iam
            .user_preferences
            .upsert(&ctx.user_id, preferences)
            .await?,
    ))
}

#[permission("org.settings.read")]
async fn get_workspace_preferences(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<UserPreferences>> {
    Ok(Json(
        state
            .iam
            .workspace_preference_defaults
            .get(&ctx.org_id)
            .await?,
    ))
}

#[permission("org.settings.manage")]
async fn update_workspace_preferences(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(mut preferences): Json<UserPreferences>,
) -> Result<Json<UserPreferences>> {
    normalize_and_validate(&mut preferences)?;
    Ok(Json(
        state
            .iam
            .workspace_preference_defaults
            .upsert(&ctx.org_id, preferences)
            .await?,
    ))
}

fn normalize_and_validate(preferences: &mut UserPreferences) -> Result<()> {
    preferences.default_home_route = preferences.default_home_route.trim().to_string();
    if !matches!(preferences.theme.as_str(), "system" | "dark" | "light") {
        return Err(Error::invalid("theme must be system, dark, or light"));
    }
    if !matches!(
        preferences.density.as_str(),
        "compact" | "normal" | "comfortable"
    ) {
        return Err(Error::invalid(
            "density must be compact, normal, or comfortable",
        ));
    }
    if !matches!(preferences.language.as_str(), "en-us" | "zh-cn") {
        return Err(Error::invalid("language must be en-us or zh-cn"));
    }
    if !matches!(preferences.time_format.as_str(), "iso_24h" | "local_12h") {
        return Err(Error::invalid("time_format must be iso_24h or local_12h"));
    }
    if !matches!(
        preferences.date_format.as_str(),
        "yyyy_mm_dd_dash" | "yyyy_mm_dd_slash" | "dd_mm_yyyy_slash" | "mm_dd_yyyy_slash"
    ) {
        return Err(Error::invalid("unsupported date_format"));
    }
    preferences.timezone = preferences.timezone.trim().to_string();
    if !is_valid_timezone(&preferences.timezone) {
        return Err(Error::invalid(
            "timezone must be empty or a valid IANA name",
        ));
    }
    if preferences.default_home_route != "last_visited"
        && (preferences.default_home_route.is_empty()
            || !preferences.default_home_route.starts_with('/')
            || preferences.default_home_route.starts_with("//"))
    {
        return Err(Error::invalid(
            "default_home_route must be last_visited or an absolute app route",
        ));
    }
    Ok(())
}

/// 空串（跟随浏览器）或轻量 IANA 形态校验——前端用下拉给合法值，这里只做防注入的字符 / 长度护栏。
fn is_valid_timezone(tz: &str) -> bool {
    if tz.is_empty() {
        return true;
    }
    tz.len() <= 64
        && tz
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '+' | '-' | '.'))
}

impl MeProfileResp {
    fn from_parts(
        user: crate::domain::iam::User,
        org: crate::domain::iam::Organization,
        display_role: String,
    ) -> Self {
        Self {
            user_id: user.id.0,
            email: user.email,
            username: user.display_name.clone(),
            display_name: user.display_name,
            avatar_url: user.avatar_url,
            bio: user.bio,
            org_id: org.id.0,
            org_name: org.name,
            org_slug: org.slug,
            display_role,
            created_at_micros: user.created_at.0,
        }
    }
}

// ===== 头像上传 / 服务 =====

/// 头像最大体积：2 MiB（前端也限制；这里做服务端硬上限）。
const AVATAR_MAX_BYTES: usize = 2 * 1024 * 1024;

/// `POST /me/avatar`：body 是原始图片字节，`Content-Type` 决定图片类型。落对象
/// 存储（即配置的后端：本地磁盘 / S3 / MinIO），并把 `user.avatar_url` 指到公开
/// 服务端点。返回更新后的 profile。
async fn upload_avatar(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<MeProfileResp>> {
    let content_type = headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let ext = avatar_ext_for_content_type(content_type)
        .ok_or_else(|| Error::invalid("avatar must be a PNG, JPEG, WEBP, or GIF image"))?;
    if body.is_empty() {
        return Err(Error::invalid("avatar file is empty"));
    }
    if body.len() > AVATAR_MAX_BYTES {
        return Err(Error::invalid("avatar must be at most 2 MiB"));
    }

    let mut user = state.iam.service.users.get(&ctx.user_id).await?;
    // 随机文件名 → 每次上传换 URL，天然绕过 <img> 缓存；路径只含安全字符。
    let file = format!("{}.{ext}", Id::new().0);
    let key = format!("avatars/{}/{}", user.id.0, file);
    let path = ObjPath::parse(&key).map_err(|e| Error::internal(format!("avatar path: {e}")))?;
    state
        .storage
        .object_store
        .put(&path, PutPayload::from(body))
        .await
        .map_err(|e| Error::internal(format!("avatar store put: {e}")))?;

    // 替换旧头像对象（best-effort，删不掉只是留点孤儿，不影响本次结果）。
    if let Some(old_key) = user
        .avatar_url
        .as_deref()
        .and_then(avatar_object_key_from_url)
        && let Ok(old_path) = ObjPath::parse(&old_key)
    {
        let _ = state.storage.object_store.delete(&old_path).await;
    }

    user.avatar_url = Some(format!("/api/v1/public/avatars/{}/{}", user.id.0, file));
    let user = state.iam.service.users.update(user).await?;
    let org = state.iam.service.orgs.get(&ctx.org_id).await?;
    Ok(Json(MeProfileResp::from_parts(user, org, ctx.display_role)))
}

/// `GET /api/v1/public/avatars/{user_id}/{file}`：公开（auth 白名单）读对象存储里
/// 的头像字节并按扩展名给 content-type。路径分量严格校验防越权 / 穿越；URL 含随机
/// id 故按 immutable 长缓存。
async fn serve_avatar(
    State(state): State<AppState>,
    Path((user_id, file)): Path<(String, String)>,
) -> std::result::Result<Response, Error> {
    if !is_safe_path_segment(&user_id) || !is_safe_avatar_file(&file) {
        return Err(Error::not_found("avatar not found"));
    }
    let ext = file.rsplit('.').next().unwrap_or_default();
    let content_type =
        avatar_content_type_for_ext(ext).ok_or_else(|| Error::not_found("avatar not found"))?;
    let key = format!("avatars/{user_id}/{file}");
    let path = ObjPath::parse(&key).map_err(|_| Error::not_found("avatar not found"))?;
    let result = state
        .storage
        .object_store
        .get(&path)
        .await
        .map_err(|_| Error::not_found("avatar not found"))?;
    let bytes = result
        .bytes()
        .await
        .map_err(|e| Error::internal(format!("avatar read: {e}")))?;
    Response::builder()
        .status(200)
        .header(CONTENT_TYPE, content_type)
        .header(CACHE_CONTROL, "public, max-age=31536000, immutable")
        .body(Body::from(bytes))
        .map_err(|e| Error::internal(format!("avatar response: {e}")))
}

/// 请求 Content-Type → 落盘扩展名（同时充当类型白名单）。
fn avatar_ext_for_content_type(content_type: &str) -> Option<&'static str> {
    match content_type.split(';').next().unwrap_or("").trim() {
        "image/png" => Some("png"),
        "image/jpeg" => Some("jpg"),
        "image/webp" => Some("webp"),
        "image/gif" => Some("gif"),
        _ => None,
    }
}

/// 扩展名 → 响应 content-type（服务端点用）。
fn avatar_content_type_for_ext(ext: &str) -> Option<&'static str> {
    match ext {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        _ => None,
    }
}

/// user_id / 文件名 stem 的安全字符集（防路径穿越 / 越权读任意对象）。
fn is_safe_path_segment(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// `<stem>.<ext>` 形态校验：stem 安全、ext 在白名单内。
fn is_safe_avatar_file(file: &str) -> bool {
    match file.rsplit_once('.') {
        Some((stem, ext)) => {
            is_safe_path_segment(stem) && matches!(ext, "png" | "jpg" | "jpeg" | "webp" | "gif")
        }
        None => false,
    }
}

/// 从存好的 avatar_url（`/api/v1/public/avatars/<user>/<file>`）反推对象 key，
/// 仅当两段都安全才返回——给 best-effort 删旧头像用。
fn avatar_object_key_from_url(url: &str) -> Option<String> {
    let rest = url.strip_prefix("/api/v1/public/avatars/")?;
    let (user, file) = rest.split_once('/')?;
    if is_safe_path_segment(user) && is_safe_avatar_file(file) {
        Some(format!("avatars/{user}/{file}"))
    } else {
        None
    }
}

fn normalize_avatar_url(value: String) -> Result<Option<String>> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() > 2048 {
        return Err(Error::invalid("avatar_url must be at most 2048 characters"));
    }
    if !(value.starts_with("https://") || value.starts_with("http://")) {
        return Err(Error::invalid("avatar_url must be an http(s) URL"));
    }
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_validation_rejects_external_home_route() {
        let mut p = UserPreferences {
            default_home_route: "https://example.com".into(),
            ..UserPreferences::default()
        };
        assert!(normalize_and_validate(&mut p).is_err());
    }

    #[test]
    fn preferences_validation_accepts_defaults() {
        let mut p = UserPreferences::default();
        assert!(normalize_and_validate(&mut p).is_ok());
    }

    #[test]
    fn preferences_validation_accepts_last_visited_home() {
        let mut p = UserPreferences {
            default_home_route: "last_visited".into(),
            ..UserPreferences::default()
        };
        assert!(normalize_and_validate(&mut p).is_ok());
    }

    #[test]
    fn preferences_validation_rejects_unknown_date_format() {
        let mut p = UserPreferences {
            date_format: "iso_magic".into(),
            ..UserPreferences::default()
        };
        assert!(normalize_and_validate(&mut p).is_err());
    }

    #[test]
    fn avatar_url_validation_normalizes_blank() {
        assert!(matches!(normalize_avatar_url("  ".into()), Ok(None)));
        assert!(normalize_avatar_url("https://example.com/me.png".into()).is_ok());
        assert!(normalize_avatar_url("file:///tmp/me.png".into()).is_err());
    }

    #[test]
    fn avatar_content_type_roundtrips_to_ext() {
        assert_eq!(avatar_ext_for_content_type("image/png"), Some("png"));
        assert_eq!(
            avatar_ext_for_content_type("image/jpeg; charset=x"),
            Some("jpg")
        );
        assert_eq!(avatar_ext_for_content_type("image/webp"), Some("webp"));
        assert_eq!(avatar_ext_for_content_type("text/html"), None);
        assert_eq!(avatar_content_type_for_ext("png"), Some("image/png"));
        assert_eq!(avatar_content_type_for_ext("jpg"), Some("image/jpeg"));
        assert_eq!(avatar_content_type_for_ext("svg"), None);
    }

    #[test]
    fn avatar_path_segments_reject_traversal() {
        assert!(is_safe_path_segment("abc123_-"));
        assert!(!is_safe_path_segment("a/b"));
        assert!(!is_safe_path_segment("a.b"));
        assert!(!is_safe_path_segment(".."));
        assert!(!is_safe_path_segment(""));
        assert!(is_safe_avatar_file("deadbeef.png"));
        assert!(!is_safe_avatar_file("deadbeef.svg"));
        assert!(!is_safe_avatar_file("../secret.png"));
        assert!(!is_safe_avatar_file("noext"));
    }

    #[test]
    fn avatar_object_key_parses_only_safe_urls() {
        assert_eq!(
            avatar_object_key_from_url("/api/v1/public/avatars/u1/abc.png").as_deref(),
            Some("avatars/u1/abc.png")
        );
        assert!(avatar_object_key_from_url("/api/v1/public/avatars/u1/../x.png").is_none());
        assert!(avatar_object_key_from_url("https://example.com/me.png").is_none());
    }
}
