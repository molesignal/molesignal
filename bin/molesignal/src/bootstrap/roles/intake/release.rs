// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Ending this process's WAL writer when the node drains (scale-in, rolling update).

use super::IntakeWorker;

impl IntakeWorker {
    /// Runs after the drain's final flush, once nothing is acknowledged any more. Every dataset
    /// whose rows are all published has its WAL objects deleted and its stream closed; then the
    /// writer is released, so another node claims whatever is left at once instead of waiting a
    /// lease.
    pub(super) async fn release_wal(&self) {
        let mut left = 0usize;

        for identity in self.wal.current_streams() {
            let flushed = match self.buffer.get(&identity.dataset_id) {
                Some(buffer) => buffer.records().lock().await.is_empty(),
                None => true,
            };

            if !flushed {
                left += 1;
                continue;
            }

            if let Err(error) = self.wal.close_flushed_stream(&identity).await {
                left += 1;
                tracing::warn!(
                    dataset_id = %identity.dataset_id,
                    %error,
                    "closing a flushed WAL stream failed; another node closes it"
                );
            }
        }

        match self.wal.release().await {
            Ok(()) if left == 0 => tracing::info!("WAL writer released; nothing left unflushed"),
            Ok(()) => tracing::warn!(
                streams = left,
                "WAL writer released with streams not fully flushed; another node publishes them \
                 from object storage"
            ),
            Err(error) => tracing::warn!(
                %error,
                "releasing the WAL writer failed; another node takes it over when the lease lapses"
            ),
        }
    }
}
