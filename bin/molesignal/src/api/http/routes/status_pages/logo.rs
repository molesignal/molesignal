// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Tenant-scoped status-page logo upload and authenticated/public delivery.

use axum::{
    Extension, Json,
    body::{Body, Bytes},
    extract::{Path, State},
    http::{
        HeaderMap,
        header::{CACHE_CONTROL, CONTENT_TYPE},
    },
    response::Response,
};
use object_store::{ObjectStoreExt, PutPayload, path::Path as ObjPath};

use crate::{
    api::{AppState, http::routes::activity_audit},
    app::iam::IamContext,
    domain::{iam::permission, status_page::StatusPage},
    shared::{Error, Result, ids::Id},
};

const LOGO_MAX_BYTES: usize = 2 * 1024 * 1024;
const PUBLIC_LOGO_PREFIX: &str = "/api/v1/public/status-pages/";

#[permission("status_pages.manage")]
pub(super) async fn upload_logo(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(page_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<StatusPage>> {
    let page_id = Id(page_id);
    let existing = state.status_pages.get_page(&ctx.org_id, &page_id).await?;
    let content_type = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let extension = extension_for_content_type(content_type)
        .ok_or_else(|| Error::invalid("logo must be a PNG, JPEG, or WEBP image"))?;
    validate_image_body(content_type, &body)?;

    let file = format!("{}.{}", Id::new().0, extension);
    let object_key = logo_object_key(&existing, &file);
    let object_path = ObjPath::parse(&object_key)
        .map_err(|error| Error::internal(format!("status page logo path: {error}")))?;
    state
        .storage
        .object_store
        .put(&object_path, PutPayload::from(body))
        .await
        .map_err(|error| Error::internal(format!("status page logo store put: {error}")))?;

    let logo_url = public_logo_url(&existing.slug, &file);
    let updated = match state
        .status_pages
        .update_logo_url(&ctx.org_id, &page_id, Some(logo_url))
        .await
    {
        Ok(page) => page,
        Err(error) => {
            let _ = state.storage.object_store.delete(&object_path).await;
            return Err(error);
        }
    };
    cleanup_replaced_logo(&state, &existing, Some(&updated)).await;
    activity_audit::record(
        &state,
        &ctx,
        "status_page.logo.update",
        "status_page",
        page_id.as_str(),
        serde_json::json!({"managed_upload": true}),
    )
    .await;
    Ok(Json(updated))
}

#[permission("status_pages.read")]
pub(super) async fn get_logo(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(page_id): Path<String>,
) -> Result<Response> {
    let page = state
        .status_pages
        .get_page(&ctx.org_id, &Id(page_id))
        .await?;
    let file =
        managed_logo_file(&page).ok_or_else(|| Error::not_found("status page logo not found"))?;
    serve_logo_object(&state, &page, &file, false).await
}

pub(super) async fn serve_public_logo(
    State(state): State<AppState>,
    Path((slug, file)): Path<(String, String)>,
) -> Result<Response> {
    if !is_safe_logo_file(&file) {
        return Err(Error::not_found("status page logo not found"));
    }
    let page = state.status_pages.get_customer_branding_page(&slug).await?;
    if managed_logo_file(&page).as_deref() != Some(file.as_str()) {
        return Err(Error::not_found("status page logo not found"));
    }
    serve_logo_object(&state, &page, &file, true).await
}

async fn serve_logo_object(
    state: &AppState,
    page: &StatusPage,
    file: &str,
    public_cache: bool,
) -> Result<Response> {
    let content_type = content_type_for_file(file)
        .ok_or_else(|| Error::not_found("status page logo not found"))?;
    let object_path = ObjPath::parse(logo_object_key(page, file))
        .map_err(|_| Error::not_found("status page logo not found"))?;
    let result = state
        .storage
        .object_store
        .get(&object_path)
        .await
        .map_err(|_| Error::not_found("status page logo not found"))?;
    let bytes = result
        .bytes()
        .await
        .map_err(|error| Error::internal(format!("status page logo read: {error}")))?;
    Response::builder()
        .status(200)
        .header(CONTENT_TYPE, content_type)
        .header(
            CACHE_CONTROL,
            if public_cache {
                "public, max-age=31536000, immutable"
            } else {
                "private, no-store"
            },
        )
        .header("x-content-type-options", "nosniff")
        .body(Body::from(bytes))
        .map_err(|error| Error::internal(format!("status page logo response: {error}")))
}

pub(super) fn rewrite_managed_logo_url(
    page: &StatusPage,
    new_slug: &str,
    requested_url: Option<String>,
) -> Option<String> {
    if requested_url.as_deref() != page.logo_url.as_deref() {
        return requested_url;
    }
    managed_logo_file(page)
        .map(|file| public_logo_url(new_slug, &file))
        .or(requested_url)
}

pub(super) async fn cleanup_replaced_logo(
    state: &AppState,
    previous: &StatusPage,
    current: Option<&StatusPage>,
) {
    let previous_key = managed_logo_object_key(previous);
    let current_key = current.and_then(managed_logo_object_key);
    if let Some(previous_key) = previous_key
        && Some(previous_key.as_str()) != current_key.as_deref()
        && let Ok(path) = ObjPath::parse(previous_key)
    {
        let _ = state.storage.object_store.delete(&path).await;
    }
}

fn validate_image_body(content_type: &str, body: &[u8]) -> Result<()> {
    if body.is_empty() {
        return Err(Error::invalid("logo file is empty"));
    }
    if body.len() > LOGO_MAX_BYTES {
        return Err(Error::invalid("logo must be at most 2 MiB"));
    }
    let valid_signature = match extension_for_content_type(content_type) {
        Some("png") => body.starts_with(b"\x89PNG\r\n\x1a\n"),
        Some("jpg") => body.starts_with(&[0xff, 0xd8, 0xff]),
        Some("webp") => body.len() >= 12 && body.starts_with(b"RIFF") && &body[8..12] == b"WEBP",
        _ => false,
    };
    if !valid_signature {
        return Err(Error::invalid(
            "logo file contents do not match its image type",
        ));
    }
    Ok(())
}

fn extension_for_content_type(content_type: &str) -> Option<&'static str> {
    match content_type.split(';').next().unwrap_or_default().trim() {
        "image/png" => Some("png"),
        "image/jpeg" => Some("jpg"),
        "image/webp" => Some("webp"),
        _ => None,
    }
}

fn content_type_for_file(file: &str) -> Option<&'static str> {
    match file.rsplit('.').next()? {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

fn is_safe_logo_file(file: &str) -> bool {
    let Some((stem, extension)) = file.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty()
        && stem.len() <= 64
        && stem
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        && matches!(extension, "png" | "jpg" | "jpeg" | "webp")
}

fn public_logo_url(slug: &str, file: &str) -> String {
    format!("{PUBLIC_LOGO_PREFIX}{slug}/logo/{file}")
}

fn managed_logo_file(page: &StatusPage) -> Option<String> {
    let prefix = format!("{PUBLIC_LOGO_PREFIX}{}/logo/", page.slug);
    let file = page.logo_url.as_deref()?.strip_prefix(&prefix)?;
    is_safe_logo_file(file).then(|| file.to_string())
}

fn logo_object_key(page: &StatusPage, file: &str) -> String {
    format!("status-page-logos/{}/{}/{}", page.org_id.0, page.id.0, file)
}

fn managed_logo_object_key(page: &StatusPage) -> Option<String> {
    managed_logo_file(page).map(|file| logo_object_key(page, &file))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::status_page::{StatusPageLifecycle, StatusPageVisibility},
        shared::time::TimestampMicros,
    };

    fn page(slug: &str, logo_url: Option<&str>) -> StatusPage {
        StatusPage {
            id: Id("page-one".into()),
            org_id: Id("org-one".into()),
            name: "Acme".into(),
            slug: slug.into(),
            logo_url: logo_url.map(str::to_string),
            brand_color: "#4F46E5".into(),
            custom_domain: None,
            timezone: "UTC".into(),
            language: "en-us".into(),
            languages: vec!["en-us".into()],
            history_days: 90,
            delivery_retention_days: 90,
            private_session_days: 7,
            visibility: StatusPageVisibility::Public,
            lifecycle: StatusPageLifecycle::Active,
            archived_at: None,
            purge_after: None,
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        }
    }

    #[test]
    fn managed_logo_keys_are_tenant_and_page_scoped() {
        let page = page(
            "acme",
            Some("/api/v1/public/status-pages/acme/logo/image.png"),
        );
        assert_eq!(
            managed_logo_object_key(&page).as_deref(),
            Some("status-page-logos/org-one/page-one/image.png")
        );
    }

    #[test]
    fn slug_change_rewrites_only_managed_logo_urls() {
        let managed = page(
            "acme",
            Some("/api/v1/public/status-pages/acme/logo/image.png"),
        );
        assert_eq!(
            rewrite_managed_logo_url(&managed, "acme-cloud", managed.logo_url.clone()).as_deref(),
            Some("/api/v1/public/status-pages/acme-cloud/logo/image.png")
        );

        let external = page("acme", Some("https://cdn.example.com/logo.svg"));
        assert_eq!(
            rewrite_managed_logo_url(&external, "acme-cloud", external.logo_url.clone()),
            external.logo_url
        );
    }

    #[test]
    fn image_signature_must_match_the_declared_type() {
        assert!(validate_image_body("image/png", b"\x89PNG\r\n\x1a\nrest").is_ok());
        assert!(validate_image_body("image/png", b"<html>not an image</html>").is_err());
        assert!(validate_image_body("image/svg+xml", b"<svg></svg>").is_err());
    }

    #[test]
    fn logo_file_rejects_path_traversal_and_unsupported_extensions() {
        assert!(is_safe_logo_file("random-id.webp"));
        assert!(!is_safe_logo_file("../secret.png"));
        assert!(!is_safe_logo_file("random-id.svg"));
        assert!(!is_safe_logo_file("no-extension"));
    }
}
