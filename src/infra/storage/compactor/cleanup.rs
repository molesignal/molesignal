// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Best-effort deletion of every object belonging to a physical parquet file.

use object_store::{ObjectStore, ObjectStoreExt, path::Path};

use super::failures;
use crate::domain::storage::ParquetFileMeta;

pub(super) async fn delete_file_outputs(store: &dyn ObjectStore, file: &ParquetFileMeta) {
    let sidecar = crate::infra::search::tantivy_index::TantivyArchive::key_for(&file.object_key);
    for object_key in std::iter::once(file.object_key.clone()).chain(sidecar) {
        if let Err(error) = store.delete(&Path::from(object_key.clone())).await
            && !matches!(error, object_store::Error::NotFound { .. })
        {
            failures().with_label_values(&["old_object_delete"]).inc();
            tracing::warn!(
                %object_key,
                %error,
                "compactor object delete failed; retention sweep will reclaim"
            );
        }
    }
}
