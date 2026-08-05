// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Object store 启动 ping + 持续探针。
//!
//! - 启动 ping：put → get → delete `_health/<uuid>` 一个 128 字节小对象；失败直接 Err
//!   让进程不启动。
//! - 后台探针：每 `health_probe_interval_secs` 跑同样 round-trip；连续 3 次失败
//!   `Probe::set_object_store_degraded(true)`，恢复后翻回 false。

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use object_store::{ObjectStore, ObjectStoreExt, PutPayload, path::Path};
use tokio::task::JoinHandle;

use crate::{
    bootstrap::roles::ingester::IngesterWorker,
    shared::{Error, Result, health::Probe},
}; // unused import to keep file compile

const PROBE_PAYLOAD: &[u8] = &[0xAB; 128];

pub async fn startup_ping(store: &dyn ObjectStore) -> Result<()> {
    let key = format!("_health/{}.probe", crate::shared::ids::Id::new());
    let path = Path::from(key.as_str());
    let started = Instant::now();
    store
        .put(&path, PutPayload::from(PROBE_PAYLOAD.to_vec()))
        .await
        .map_err(|e| Error::internal(format!("startup_ping put: {e}")))?;
    let got = store
        .get(&path)
        .await
        .map_err(|e| Error::internal(format!("startup_ping get: {e}")))?
        .bytes()
        .await
        .map_err(|e| Error::internal(format!("startup_ping read: {e}")))?;
    if got.as_ref() != PROBE_PAYLOAD {
        return Err(Error::internal("startup_ping read mismatch"));
    }
    let _ = store.delete(&path).await; // best-effort
    crate::infra::storage::object::production::health_dur()
        .with_label_values(&["startup"])
        .observe(started.elapsed().as_secs_f64());
    Ok(())
}

pub fn spawn_probe(
    store: Arc<dyn ObjectStore>,
    probe: Arc<Probe>,
    interval_secs: u32,
) -> JoinHandle<()> {
    let interval = Duration::from_secs(interval_secs.max(5) as u64);
    tokio::spawn(async move {
        let mut consecutive_fails = 0u32;
        let mut ticker = tokio::time::interval(interval);
        ticker.tick().await; // skip first
        loop {
            ticker.tick().await;
            let started = Instant::now();
            let key = format!("_health/{}.probe", crate::shared::ids::Id::new());
            let path = Path::from(key.as_str());
            let attempt = async {
                store
                    .put(&path, PutPayload::from(PROBE_PAYLOAD.to_vec()))
                    .await?;
                let bytes = store.get(&path).await?.bytes().await?;
                if bytes.as_ref() != PROBE_PAYLOAD {
                    return Err(object_store::Error::Generic {
                        store: "probe",
                        source: "probe payload mismatch".to_string().into(),
                    });
                }
                let _ = store.delete(&path).await;
                Ok::<_, object_store::Error>(())
            }
            .await;
            crate::infra::storage::object::production::health_dur()
                .with_label_values(&["probe"])
                .observe(started.elapsed().as_secs_f64());
            match attempt {
                Ok(_) => {
                    if consecutive_fails > 0 {
                        tracing::info!("object_store health recovered");
                    }
                    consecutive_fails = 0;
                    probe.set_object_store_degraded(false);
                }
                Err(e) => {
                    consecutive_fails += 1;
                    tracing::warn!(fails = consecutive_fails, error = %e, "object_store probe failed");
                    if consecutive_fails >= 3 {
                        probe.set_object_store_degraded(true);
                    }
                }
            }
        }
    })
}

// 避免 unused import 警告
const _: fn() = || {
    let _ = std::any::TypeId::of::<IngesterWorker>;
};
