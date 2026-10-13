// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 按组织限定的只读对象存储视图：存储层的前缀隔离。
//!
//! 对象 key 由 [`StorageLayout`] 统一生成，组织 ID 是 key 的一部分
//! （`artifacts/v1/{org}/…`、`manifests/v1/{org}/…`）。共享的读句柄本身不拦截越界读取：
//! 只要拿到别的组织的 key，原样就能读到。查询路径的组织隔离因此只剩一个前提——Catalog
//! 只返回本组织的文件；节点间分片查询的文件列表来自远端，同样不受这个前提保护。
//!
//! [`scoped_to_org`] 把共享读句柄包成只能读本组织前缀的视图：越界的读取与列举返回
//! `PermissionDenied`，所有写入类操作一律拒绝。查询按请求的组织包一次即可，共享句柄与
//! 后台任务不受影响。
//!
//! 没有组织前缀可授权时（空组织 ID、含 `/` 的组织 ID）视图拒绝一切访问，而不是放行整个
//! `artifacts/v1/` 命名空间。拒绝信息与日志都不带被拒绝的 key，避免把其它组织的标识带出去。
//!
//! 视图只放行 Catalog 对象的两个前缀。Search Job 结果（`search_jobs/v1/{org}/…`）的 key 里也有
//! 组织 ID，但它不是查询要读的数据：任务服务按任务行上记的 key 用共享句柄读它，视图不放行。

use std::{fmt, ops::Range, sync::Arc};

use async_trait::async_trait;
use bytes::Bytes;
use futures::{StreamExt, stream::BoxStream};
use object_store::{
    CopyOptions, GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta, ObjectStore,
    PutMultipartOptions, PutOptions, PutPayload, PutResult, RenameOptions, path::Path,
};

use super::layout::StorageLayout;
use crate::shared::ids::Id;

/// 把共享读存储限定到 `organization_id` 的对象前缀。
pub fn scoped_to_org(inner: Arc<dyn ObjectStore>, organization_id: &Id) -> Arc<dyn ObjectStore> {
    Arc::new(OrgScopedReadStore::new(inner, organization_id))
}

/// 只读、限定在单个组织前缀内的对象存储视图。
pub struct OrgScopedReadStore {
    inner: Arc<dyn ObjectStore>,
    organization: String,
    prefixes: Vec<Path>,
}

impl OrgScopedReadStore {
    pub fn new(inner: Arc<dyn ObjectStore>, organization_id: &Id) -> Self {
        let id = organization_id.as_str();

        // `Path` 会丢掉空路径段：空 ID 的前缀会退化成整个 `artifacts/v1/`，带分隔符的 ID
        // 又对应不到唯一的组织目录。两种情况都不授权任何前缀。
        let prefixes = if id.is_empty() || id.contains('/') {
            Vec::new()
        } else {
            StorageLayout::catalog_scan_prefixes_for(organization_id)
                .into_iter()
                .map(|prefix| Path::from(prefix.trim_end_matches('/')))
                .collect()
        };

        Self {
            inner,
            organization: id.to_owned(),
            prefixes,
        }
    }

    /// `location` 是否落在本组织的某个前缀之内。按路径段匹配：`org-a` 不匹配 `org-ab`。
    fn owns(&self, location: &Path) -> bool {
        self.prefixes
            .iter()
            .any(|prefix| location.prefix_matches(prefix))
    }

    /// 列举的起点必须整体落在本组织前缀内；不带前缀等于列举整个存储，一律拒绝。
    fn owns_listing(&self, prefix: Option<&Path>) -> bool {
        prefix.is_some_and(|prefix| self.owns(prefix))
    }

    fn check(&self, operation: &'static str, location: &Path) -> object_store::Result<()> {
        if self.owns(location) {
            return Ok(());
        }

        Err(self.denied(operation))
    }

    fn denied(&self, operation: &'static str) -> object_store::Error {
        tracing::warn!(
            organization = %self.organization,
            operation,
            "object outside the organization's storage prefix was denied"
        );

        object_store::Error::PermissionDenied {
            path: "<outside organization prefix>".to_owned(),
            source: "object is outside the organization's storage prefix".into(),
        }
    }

    fn denied_listing(&self) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
        futures::stream::once(std::future::ready(Err(self.denied("list")))).boxed()
    }
}

impl fmt::Debug for OrgScopedReadStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OrgScopedReadStore")
            .finish_non_exhaustive()
    }
}

impl fmt::Display for OrgScopedReadStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MoleSignalOrgScopedReadStore")
    }
}

#[async_trait]
#[deny(clippy::missing_trait_methods)]
impl ObjectStore for OrgScopedReadStore {
    async fn put_opts(
        &self,
        _location: &Path,
        _payload: PutPayload,
        _options: PutOptions,
    ) -> object_store::Result<PutResult> {
        Err(read_only("put"))
    }

    async fn put_multipart_opts(
        &self,
        _location: &Path,
        _options: PutMultipartOptions,
    ) -> object_store::Result<Box<dyn MultipartUpload>> {
        Err(read_only("put_multipart"))
    }

    async fn get_opts(
        &self,
        location: &Path,
        options: GetOptions,
    ) -> object_store::Result<GetResult> {
        self.check("get", location)?;

        self.inner.get_opts(location, options).await
    }

    async fn get_ranges(
        &self,
        location: &Path,
        ranges: &[Range<u64>],
    ) -> object_store::Result<Vec<Bytes>> {
        self.check("get_ranges", location)?;

        self.inner.get_ranges(location, ranges).await
    }

    fn delete_stream(
        &self,
        locations: BoxStream<'static, object_store::Result<Path>>,
    ) -> BoxStream<'static, object_store::Result<Path>> {
        locations
            .map(|location| location.and_then(|_| Err(read_only("delete"))))
            .boxed()
    }

    fn list(&self, prefix: Option<&Path>) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
        if !self.owns_listing(prefix) {
            return self.denied_listing();
        }

        self.inner.list(prefix)
    }

    fn list_with_offset(
        &self,
        prefix: Option<&Path>,
        offset: &Path,
    ) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
        if !self.owns_listing(prefix) {
            return self.denied_listing();
        }

        self.inner.list_with_offset(prefix, offset)
    }

    async fn list_with_delimiter(&self, prefix: Option<&Path>) -> object_store::Result<ListResult> {
        if !self.owns_listing(prefix) {
            return Err(self.denied("list"));
        }

        self.inner.list_with_delimiter(prefix).await
    }

    async fn copy_opts(
        &self,
        _from: &Path,
        _to: &Path,
        _options: CopyOptions,
    ) -> object_store::Result<()> {
        Err(read_only("copy"))
    }

    async fn rename_opts(
        &self,
        _from: &Path,
        _to: &Path,
        _options: RenameOptions,
    ) -> object_store::Result<()> {
        Err(read_only("rename"))
    }
}

fn read_only(operation: &str) -> object_store::Error {
    object_store::Error::NotImplemented {
        operation: operation.to_owned(),
        implementer: "OrgScopedReadStore".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use futures::TryStreamExt;
    use object_store::{ObjectStoreExt as _, PutPayload, memory::InMemory};

    use super::*;

    const OWN_DATA: &str = "artifacts/v1/org-a/logs/ds/p-0-00/seg/a.parquet";
    const OWN_MANIFEST: &str = "manifests/v1/org-a/logs/ds/p-0-00/1.parquet";
    const SIBLING_DATA: &str = "artifacts/v1/org-ab/logs/ds/p-0-00/seg/a.parquet";
    const OTHER_DATA: &str = "artifacts/v1/org-b/logs/ds/p-0-00/seg/a.parquet";
    const OTHER_MANIFEST: &str = "manifests/v1/org-b/logs/ds/p-0-00/1.parquet";
    const UNRELATED: &str = "chat-archives/org-a/transcript.json";

    async fn populated() -> Arc<dyn ObjectStore> {
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());

        for key in [
            OWN_DATA,
            OWN_MANIFEST,
            SIBLING_DATA,
            OTHER_DATA,
            OTHER_MANIFEST,
            UNRELATED,
        ] {
            store
                .put(&Path::from(key), PutPayload::from_static(b"0123456789"))
                .await
                .unwrap();
        }

        store
    }

    async fn scoped(org: &str) -> Arc<dyn ObjectStore> {
        scoped_to_org(populated().await, &Id::from_string(org))
    }

    fn is_denied(error: &object_store::Error) -> bool {
        matches!(error, object_store::Error::PermissionDenied { .. })
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reads_inside_the_organization_prefix_succeed() {
        let store = scoped("org-a").await;
        let data = Path::from(OWN_DATA);

        assert_eq!(
            store.get(&data).await.unwrap().bytes().await.unwrap().len(),
            10
        );
        assert_eq!(store.head(&data).await.unwrap().size, 10);
        assert_eq!(store.get_range(&data, 2..5).await.unwrap().len(), 3);
        assert_eq!(
            store.get_ranges(&data, &[0..2, 8..10]).await.unwrap().len(),
            2
        );

        // 清单对象同属本组织前缀。
        assert_eq!(
            store.head(&Path::from(OWN_MANIFEST)).await.unwrap().size,
            10
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reads_outside_the_organization_prefix_are_denied() {
        let store = scoped("org-a").await;

        for key in [OTHER_DATA, OTHER_MANIFEST, SIBLING_DATA, UNRELATED] {
            let location = Path::from(key);

            assert!(
                is_denied(&store.get(&location).await.unwrap_err()),
                "get {key}"
            );
            assert!(
                is_denied(&store.head(&location).await.unwrap_err()),
                "head {key}"
            );
            assert!(
                is_denied(&store.get_range(&location, 0..4).await.unwrap_err()),
                "get_range {key}"
            );
            assert!(
                is_denied(
                    &store
                        .get_ranges(&location, std::slice::from_ref(&(0..4)))
                        .await
                        .unwrap_err()
                ),
                "get_ranges {key}"
            );
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn denial_does_not_reveal_the_requested_key() {
        let store = scoped("org-a").await;

        let error = store.get(&Path::from(OTHER_DATA)).await.unwrap_err();

        assert!(!error.to_string().contains("org-b"), "{error}");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn listing_is_limited_to_the_organization_prefix() {
        let store = scoped("org-a").await;
        let own = Path::from("artifacts/v1/org-a");

        let listed: Vec<_> = store.list(Some(&own)).try_collect().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].location.as_ref(), OWN_DATA);

        let delimited = store.list_with_delimiter(Some(&own)).await.unwrap();
        assert_eq!(delimited.common_prefixes.len(), 1);

        let offset = Path::from("artifacts/v1/org-a");
        let after: Vec<_> = store
            .list_with_offset(Some(&own), &offset)
            .try_collect()
            .await
            .unwrap();
        assert_eq!(after.len(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn listing_outside_the_organization_prefix_is_denied() {
        let store = scoped("org-a").await;
        let outside = [
            None,
            Some(Path::from("artifacts/v1")),
            Some(Path::from("artifacts/v1/org-b")),
            Some(Path::from("artifacts/v1/org-ab")),
            Some(Path::from("chat-archives")),
        ];

        for prefix in &outside {
            let first = store.list(prefix.as_ref()).try_next().await;
            assert!(is_denied(&first.unwrap_err()), "list {prefix:?}");

            let offset = Path::from("a");
            let first = store
                .list_with_offset(prefix.as_ref(), &offset)
                .try_next()
                .await;
            assert!(
                is_denied(&first.unwrap_err()),
                "list_with_offset {prefix:?}"
            );

            let delimited = store.list_with_delimiter(prefix.as_ref()).await;
            assert!(
                is_denied(&delimited.unwrap_err()),
                "list_with_delimiter {prefix:?}"
            );
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn search_job_results_are_not_served_through_the_view() {
        let inner = populated().await;
        let org = Id::from_string("org-a");
        let key = StorageLayout::search_job_result_key(&org, &Id::from_string("job"), 0);
        let location = Path::from(key.as_str());

        inner
            .put(&location, PutPayload::from_static(b"{}\n"))
            .await
            .unwrap();

        let store = scoped_to_org(inner, &org);

        assert!(is_denied(&store.get(&location).await.unwrap_err()));
        assert!(is_denied(&store.head(&location).await.unwrap_err()));

        for prefix in ["search_jobs", "search_jobs/v1", "search_jobs/v1/org-a"] {
            let first = store.list(Some(&Path::from(prefix))).try_next().await;
            assert!(is_denied(&first.unwrap_err()), "list {prefix}");
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mutations_are_rejected_even_inside_the_prefix() {
        let store = scoped("org-a").await;
        let own = Path::from(OWN_DATA);
        let target = Path::from("artifacts/v1/org-a/logs/ds/p-0-00/seg/copy.parquet");

        let put = store.put(&own, PutPayload::from_static(b"x")).await;
        assert!(matches!(
            put,
            Err(object_store::Error::NotImplemented { .. })
        ));

        let multipart = store.put_multipart(&own).await;
        assert!(matches!(
            multipart,
            Err(object_store::Error::NotImplemented { .. })
        ));

        let delete = store.delete(&own).await;
        assert!(matches!(
            delete,
            Err(object_store::Error::NotImplemented { .. })
        ));

        let copy = store.copy(&own, &target).await;
        assert!(matches!(
            copy,
            Err(object_store::Error::NotImplemented { .. })
        ));

        let rename = store.rename(&own, &target).await;
        assert!(matches!(
            rename,
            Err(object_store::Error::NotImplemented { .. })
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn an_unusable_organization_id_grants_nothing() {
        // 空 ID 若被当作前缀，会放行整个 `artifacts/v1/` 命名空间。
        for org in ["", "org-a/logs", "a/b"] {
            let store = scoped(org).await;

            for key in [OWN_DATA, OTHER_DATA, OWN_MANIFEST] {
                let error = store.get(&Path::from(key)).await.unwrap_err();
                assert!(is_denied(&error), "org {org:?} read {key}");
            }

            let listed = store
                .list(Some(&Path::from("artifacts/v1")))
                .try_next()
                .await;
            assert!(is_denied(&listed.unwrap_err()), "org {org:?} list");
        }
    }
}
