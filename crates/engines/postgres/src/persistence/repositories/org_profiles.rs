// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 组织的门面：组织切换里认出一个组织用的 logo 和一两行描述，存在 `org_settings` 的 `org.profile`
//! 一节。logo 是网页端缩成小图后的 `data:` 地址，跟组织列表一起下发，不再单独请求。

use std::collections::HashMap;

use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row, types::Json};

use super::{
    org_settings::{self, sections::ORG_PROFILE},
    sqlx_err,
};

/// logo 解码后最多这么大：界面最大只画 44px 见方，网页端上传前先缩到 128px。
pub const MAX_LOGO_BYTES: usize = 64 * 1024;
/// 描述最多这么多字：组织切换里最多显示两行。
pub const MAX_DESCRIPTION_CHARS: usize = 120;

/// 一个组织的门面。都没设置时组织切换画平台的 logo，不显示描述。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrgProfile {
    /// `data:image/png;base64,…` 这样的地址。
    #[serde(default)]
    pub logo: Option<String>,
    #[serde(default)]
    pub description: String,
}

impl OrgProfile {
    /// 去掉两端的空白，空的 logo 当作没有，再看 logo 与描述合不合要求。
    pub fn normalize(mut self) -> Result<Self> {
        self.description = self.description.trim().to_owned();

        if self.description.chars().count() > MAX_DESCRIPTION_CHARS {
            return Err(Error::invalid(format!(
                "description: at most {MAX_DESCRIPTION_CHARS} characters"
            )));
        }

        if self.description.contains(['\n', '\r']) {
            return Err(Error::invalid(
                "description: one paragraph, without line breaks",
            ));
        }

        self.logo = self
            .logo
            .map(|logo| logo.trim().to_owned())
            .filter(|logo| !logo.is_empty());

        if let Some(logo) = &self.logo {
            validate_logo(logo)?;
        }

        Ok(self)
    }
}

/// logo 得是 PNG、JPEG 或 WEBP 的 base64 `data:` 地址，内容和它说的格式对得上，解码后不超过上限。
fn validate_logo(logo: &str) -> Result<()> {
    let (mime, data) = logo
        .strip_prefix("data:")
        .and_then(|rest| rest.split_once(";base64,"))
        .ok_or_else(|| Error::invalid("logo: a base64 data URL"))?;

    // 先按长度挡掉解码后必然超限的，免得为它分配内存。
    if data.len() > MAX_LOGO_BYTES.div_ceil(3) * 4 {
        return Err(too_large());
    }

    let bytes = STANDARD
        .decode(data)
        .map_err(|_| Error::invalid("logo: not valid base64"))?;

    if bytes.len() > MAX_LOGO_BYTES {
        return Err(too_large());
    }

    let matches = match mime {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/webp" => bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP",
        _ => return Err(Error::invalid("logo: a PNG, JPEG or WEBP image")),
    };

    if matches {
        Ok(())
    } else {
        Err(Error::invalid("logo: the image is not what its type says"))
    }
}

fn too_large() -> Error {
    Error::invalid(format!("logo: at most {} KiB", MAX_LOGO_BYTES / 1024))
}

#[async_trait]
pub trait OrgProfileRepository: Send + Sync {
    async fn get(&self, org_id: &Id) -> Result<OrgProfile>;
    /// 这些组织里写过门面的，按组织 id。
    async fn among(&self, org_ids: &[Id]) -> Result<HashMap<Id, OrgProfile>>;
    async fn put(
        &self,
        org_id: &Id,
        profile: &OrgProfile,
        updated_by: &Id,
        at: TimestampMicros,
    ) -> Result<()>;
}

pub struct PgOrgProfileRepository {
    pool: PgPool,
}

impl PgOrgProfileRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl OrgProfileRepository for PgOrgProfileRepository {
    async fn get(&self, org_id: &Id) -> Result<OrgProfile> {
        Ok(org_settings::read(&self.pool, &org_id.0, ORG_PROFILE)
            .await?
            .map(|section| section.value)
            .unwrap_or_default())
    }

    async fn among(&self, org_ids: &[Id]) -> Result<HashMap<Id, OrgProfile>> {
        if org_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let ids = org_ids.iter().map(|id| id.0.clone()).collect::<Vec<_>>();

        let rows = sqlx::query(
            "SELECT org_id, value FROM org_settings
             WHERE section = $2 AND org_id = ANY($1::TEXT[])",
        )
        .bind(&ids)
        .bind(ORG_PROFILE)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter()
            .map(|row| {
                let org_id = row.try_get::<String, _>("org_id").map_err(sqlx_err)?;
                let profile = row
                    .try_get::<Json<OrgProfile>, _>("value")
                    .map_err(sqlx_err)?
                    .0;

                Ok((Id::from_string(org_id), profile))
            })
            .collect()
    }

    async fn put(
        &self,
        org_id: &Id,
        profile: &OrgProfile,
        updated_by: &Id,
        at: TimestampMicros,
    ) -> Result<()> {
        org_settings::write(
            &self.pool,
            &org_id.0,
            ORG_PROFILE,
            profile,
            Some(&updated_by.0),
            at,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

    fn data_url(mime: &str, bytes: &[u8]) -> String {
        format!("data:{mime};base64,{}", STANDARD.encode(bytes))
    }

    fn profile(logo: Option<String>, description: &str) -> OrgProfile {
        OrgProfile {
            logo,
            description: description.to_owned(),
        }
    }

    #[test]
    fn keeps_a_png_logo_and_trims_the_description() {
        let logo = data_url("image/png", PNG);
        let normalized = profile(Some(logo.clone()), "  Payments in production  ")
            .normalize()
            .unwrap();

        assert_eq!(normalized.logo, Some(logo));
        assert_eq!(normalized.description, "Payments in production");
    }

    #[test]
    fn an_empty_logo_is_no_logo() {
        let normalized = profile(Some("  ".into()), "").normalize().unwrap();

        assert_eq!(normalized.logo, None);
    }

    #[test]
    fn refuses_a_logo_that_is_not_a_data_url() {
        let error = profile(Some("https://example.com/logo.png".into()), "").normalize();

        assert!(error.is_err());
    }

    #[test]
    fn refuses_an_image_type_it_does_not_take() {
        let error = profile(Some(data_url("image/svg+xml", b"<svg/>")), "").normalize();

        assert!(error.is_err());
    }

    #[test]
    fn refuses_bytes_that_are_not_what_the_type_says() {
        let error = profile(Some(data_url("image/png", b"GIF89a")), "").normalize();

        assert!(error.is_err());
    }

    #[test]
    fn refuses_a_logo_over_the_limit() {
        let mut bytes = PNG.to_vec();
        bytes.resize(MAX_LOGO_BYTES + 1, 0);

        let error = profile(Some(data_url("image/png", &bytes)), "").normalize();

        assert!(error.is_err());
    }

    #[test]
    fn refuses_a_long_or_broken_description() {
        let long = "字".repeat(MAX_DESCRIPTION_CHARS + 1);

        assert!(profile(None, &long).normalize().is_err());
        assert!(profile(None, "first\nsecond").normalize().is_err());
    }
}
