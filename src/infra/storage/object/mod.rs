// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Arc;

use object_store::{
    ObjectStore, aws::AmazonS3Builder, azure::MicrosoftAzureBuilder,
    gcp::GoogleCloudStorageBuilder, local::LocalFileSystem,
};

use crate::{
    config::ObjectStoreSettings,
    shared::{Error, Result},
};

pub mod credentials;
pub mod production;

/// 按 `[object_store]` 配置构造对象存储客户端。
/// 支持四种 backend：local / s3 / azure / gcs。
pub fn build(cfg: &ObjectStoreSettings) -> Result<Arc<dyn ObjectStore>> {
    match cfg.backend.as_str() {
        "local" => build_local(cfg),
        "s3" => build_s3(cfg),
        "azure" => build_azure(cfg),
        "gcs" => build_gcs(cfg),
        other => Err(Error::invalid(format!(
            "unsupported object_store backend: {other}"
        ))),
    }
}

fn build_local(cfg: &ObjectStoreSettings) -> Result<Arc<dyn ObjectStore>> {
    std::fs::create_dir_all(&cfg.root)
        .map_err(|e| Error::internal(format!("create local object_store root: {e}")))?;
    let fs = LocalFileSystem::new_with_prefix(&cfg.root)
        .map_err(|e| Error::internal(format!("local object_store build: {e}")))?;
    Ok(Arc::new(fs))
}

fn build_s3(cfg: &ObjectStoreSettings) -> Result<Arc<dyn ObjectStore>> {
    if cfg.bucket.is_empty() {
        return Err(Error::invalid("object_store.bucket required for s3"));
    }
    let mut b = AmazonS3Builder::new().with_bucket_name(&cfg.bucket);
    if !cfg.region.is_empty() {
        b = b.with_region(&cfg.region);
    }
    if !cfg.endpoint.is_empty() {
        b = b.with_endpoint(&cfg.endpoint).with_allow_http(true);
    }
    if !cfg.access_key.is_empty() {
        b = b.with_access_key_id(&cfg.access_key);
    }
    if !cfg.secret_key.is_empty() {
        b = b.with_secret_access_key(&cfg.secret_key);
    }
    if cfg.path_style {
        b = b.with_virtual_hosted_style_request(false)
    }
    let store = b
        .build()
        .map_err(|e| Error::internal(format!("s3 object_store build: {e}")))?;
    Ok(Arc::new(store))
}

fn build_azure(cfg: &ObjectStoreSettings) -> Result<Arc<dyn ObjectStore>> {
    if cfg.bucket.is_empty() {
        return Err(Error::invalid("object_store.bucket required for azure"));
    }
    // Azure 的 account / access_key 优先走环境变量
    // AZURE_STORAGE_ACCOUNT / AZURE_STORAGE_ACCESS_KEY；显式配置覆盖之。
    let mut b = MicrosoftAzureBuilder::from_env().with_container_name(&cfg.bucket);
    if !cfg.access_key.is_empty() {
        b = b.with_access_key(&cfg.access_key);
    }
    let store = b
        .build()
        .map_err(|e| Error::internal(format!("azure object_store build: {e}")))?;
    Ok(Arc::new(store))
}

fn build_gcs(cfg: &ObjectStoreSettings) -> Result<Arc<dyn ObjectStore>> {
    if cfg.bucket.is_empty() {
        return Err(Error::invalid("object_store.bucket required for gcs"));
    }
    // GCS 凭据走 GOOGLE_APPLICATION_CREDENTIALS（指向 service account json）。
    let store = GoogleCloudStorageBuilder::from_env()
        .with_bucket_name(&cfg.bucket)
        .build()
        .map_err(|e| Error::internal(format!("gcs object_store build: {e}")))?;
    Ok(Arc::new(store))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_round_trip() {
        let tmp = tempfile::tempdir().expect("tmpdir");
        let cfg = ObjectStoreSettings {
            backend: "local".into(),
            root: tmp.path().to_string_lossy().into(),
            ..Default::default()
        };
        let _store = build(&cfg).expect("build local");
    }

    #[test]
    fn unknown_backend_rejected() {
        let cfg = ObjectStoreSettings {
            backend: "foo".into(),
            ..Default::default()
        };
        let err = build(&cfg).unwrap_err();
        assert!(err.to_string().contains("unsupported object_store backend"));
    }

    #[test]
    fn s3_requires_bucket() {
        let cfg = ObjectStoreSettings {
            backend: "s3".into(),
            ..Default::default()
        };
        assert!(build(&cfg).is_err());
    }
}
