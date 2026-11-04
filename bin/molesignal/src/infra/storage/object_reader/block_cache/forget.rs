// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::{RangeBlockCache, RegisteredObject, block_key};

impl RangeBlockCache {
    /// Drops every cached block of an object deleted from the store, in memory and on disk, and
    /// returns how many block files were removed. Block keys follow from the object's identity
    /// and block index, so no directory is scanned.
    pub(in crate::infra::storage::object_reader) async fn forget(
        &self,
        registered: &RegisteredObject,
    ) -> usize {
        let blocks = registered.object.size_bytes.div_ceil(self.block_size);
        let mut removed = 0;

        for index in 0..blocks {
            let key = block_key(registered, index);

            if let Some(memory) = &self.memory {
                memory.invalidate(&key);
            }

            let cached = self.index.lock().entries.contains_key(&key);

            if cached {
                self.remove_block(&key, &self.path_for(&key)).await;
                removed += 1;
            }
        }

        removed
    }
}
