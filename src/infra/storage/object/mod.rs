// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Arc;

use object_store::{
    ObjectStore, aws::AmazonS3Builder, azure::MicrosoftAzureBuilder,
    gcp::GoogleCloudStorageBuilder, local::LocalFileSystem,
};
use url::Url;

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
    let bucket = cfg.bucket.trim();
    if bucket.is_empty() {
        return Err(Error::invalid("object_store.bucket required for s3"));
    }

    let mut b = AmazonS3Builder::new()
        .with_bucket_name(bucket)
        .with_virtual_hosted_style_request(!cfg.path_style);
    if !cfg.region.trim().is_empty() {
        b = b.with_region(cfg.region.trim());
    }
    if let Some(endpoint) = prepare_s3_endpoint(cfg, bucket)? {
        b = b.with_endpoint(endpoint.url);
        if endpoint.allow_http {
            b = b.with_allow_http(true);
        }
    }
    if !cfg.access_key.is_empty() {
        b = b.with_access_key_id(&cfg.access_key);
    }
    if !cfg.secret_key.is_empty() {
        b = b.with_secret_access_key(&cfg.secret_key);
    }
    let store = b
        .build()
        .map_err(|e| Error::internal(format!("s3 object_store build: {e}")))?;
    Ok(Arc::new(store))
}

#[derive(Debug, PartialEq, Eq)]
struct PreparedS3Endpoint {
    url: String,
    allow_http: bool,
}

/// `object_store` requires a bucket-specific endpoint for virtual-hosted requests.
/// Validate and normalize it here so malformed configuration is rejected before
/// the upstream SigV4 signer attempts to unwrap an invalid HTTP request.
fn prepare_s3_endpoint(
    cfg: &ObjectStoreSettings,
    bucket: &str,
) -> Result<Option<PreparedS3Endpoint>> {
    let raw = cfg.endpoint.trim();
    if raw.is_empty() {
        return Ok(None);
    }

    let mut endpoint = Url::parse(raw).map_err(|e| {
        Error::invalid(format!(
            "object_store.endpoint must be an absolute HTTP(S) URL: {e}"
        ))
    })?;
    if !matches!(endpoint.scheme(), "http" | "https") {
        return Err(Error::invalid(
            "object_store.endpoint scheme must be http or https",
        ));
    }
    let host = endpoint
        .host_str()
        .ok_or_else(|| Error::invalid("object_store.endpoint must include a host"))?
        .to_owned();
    if !endpoint.username().is_empty() || endpoint.password().is_some() {
        return Err(Error::invalid(
            "object_store.endpoint must not include credentials",
        ));
    }
    if endpoint.query().is_some() || endpoint.fragment().is_some() {
        return Err(Error::invalid(
            "object_store.endpoint must not include a query or fragment",
        ));
    }

    if !cfg.path_style {
        let expected_prefix = format!("{}.", bucket.to_ascii_lowercase());
        if !host.to_ascii_lowercase().starts_with(&expected_prefix) {
            let virtual_host = format!("{bucket}.{host}");
            endpoint.set_host(Some(&virtual_host)).map_err(|e| {
                Error::invalid(format!(
                    "object_store bucket cannot be used in a virtual-hosted endpoint: {e}"
                ))
            })?;
        }
    }

    let allow_http = endpoint.scheme() == "http";
    let url = endpoint.as_str().trim_end_matches('/').to_owned();
    let request_url = if cfg.path_style {
        format!("{url}/{bucket}/_health/probe")
    } else {
        format!("{url}/_health/probe")
    };
    request_url.parse::<http::Uri>().map_err(|e| {
        Error::invalid(format!(
            "object_store endpoint produces an invalid request URI: {e}"
        ))
    })?;

    Ok(Some(PreparedS3Endpoint { url, allow_http }))
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

    #[test]
    fn s3_virtual_hosted_endpoint_includes_bucket() {
        let cfg = ObjectStoreSettings {
            backend: "s3".into(),
            bucket: "molesignal".into(),
            endpoint: "https://obs.ap-southeast-3.myhuaweicloud.com/".into(),
            ..Default::default()
        };

        let endpoint = prepare_s3_endpoint(&cfg, &cfg.bucket)
            .expect("valid endpoint")
            .expect("custom endpoint");

        assert_eq!(
            endpoint,
            PreparedS3Endpoint {
                url: "https://molesignal.obs.ap-southeast-3.myhuaweicloud.com".into(),
                allow_http: false,
            }
        );
    }

    #[test]
    fn s3_path_style_endpoint_keeps_service_host() {
        let cfg = ObjectStoreSettings {
            backend: "s3".into(),
            bucket: "molesignal".into(),
            endpoint: "http://minio:9000/".into(),
            path_style: true,
            ..Default::default()
        };

        let endpoint = prepare_s3_endpoint(&cfg, &cfg.bucket)
            .expect("valid endpoint")
            .expect("custom endpoint");

        assert_eq!(
            endpoint,
            PreparedS3Endpoint {
                url: "http://minio:9000".into(),
                allow_http: true,
            }
        );
    }

    #[test]
    fn s3_malformed_endpoint_is_rejected_without_panicking() {
        let cfg = ObjectStoreSettings {
            backend: "s3".into(),
            bucket: "molesignal".into(),
            endpoint: "\"https://obs.example.com\"".into(),
            ..Default::default()
        };

        let err = build(&cfg).expect_err("quoted endpoint must be rejected");
        assert!(err.to_string().contains("absolute HTTP(S) URL"));
    }
}
