// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 后端请求的并发许可：一个请求占一个许可，从第一次尝试到最后一次结束。等了多久、占了多久、
//! 此刻有几个在等、几个在用都记成指标；等到期限还没拿到时，这次请求以 `concurrency_wait` 失败。

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use object_store::Result as ObjectStoreResult;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use super::telemetry::{
    errors_total, permit_hold, permit_wait, permits_in_use, permits_waiting, timeout_error,
};

#[derive(Clone)]
pub(super) struct Permits {
    semaphore: Arc<Semaphore>,
    backend: &'static str,
}

impl Permits {
    pub(super) fn new(backend: &'static str, permits: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(permits)),
            backend,
        }
    }

    /// 至多等 `deadline` 拿一个许可。
    pub(super) async fn acquire(
        &self,
        operation: &'static str,
        deadline: Duration,
    ) -> ObjectStoreResult<Held> {
        let waiting = Waiting::start(self.backend);
        let asked = Instant::now();
        let acquired = tokio::time::timeout(deadline, self.semaphore.clone().acquire_owned()).await;

        permit_wait()
            .with_label_values(&[self.backend, operation])
            .observe(asked.elapsed().as_secs_f64());

        drop(waiting);

        let permit = match acquired {
            Ok(Ok(permit)) => permit,
            Ok(Err(_)) => return Err(timeout_error(self.backend, "concurrency_closed")),
            Err(_) => {
                errors_total()
                    .with_label_values(&[self.backend, operation, "permit_wait"])
                    .inc();

                return Err(timeout_error(self.backend, "concurrency_wait"));
            }
        };

        permits_in_use().with_label_values(&[self.backend]).inc();

        Ok(Held {
            _permit: permit,
            backend: self.backend,
            operation,
            since: Instant::now(),
        })
    }
}

/// 拿到的许可；丢掉时记下占了多久并归还。
pub(super) struct Held {
    _permit: OwnedSemaphorePermit,
    backend: &'static str,
    operation: &'static str,
    since: Instant,
}

impl Drop for Held {
    fn drop(&mut self) {
        permit_hold()
            .with_label_values(&[self.backend, self.operation])
            .observe(self.since.elapsed().as_secs_f64());

        permits_in_use().with_label_values(&[self.backend]).dec();
    }
}

/// 在等许可的请求数。等的 future 被丢掉（比如查询取消）时也减回去。
struct Waiting(&'static str);

impl Waiting {
    fn start(backend: &'static str) -> Self {
        permits_waiting().with_label_values(&[backend]).inc();
        Self(backend)
    }
}

impl Drop for Waiting {
    fn drop(&mut self) {
        permits_waiting().with_label_values(&[self.0]).dec();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEADLINE: Duration = Duration::from_millis(40);

    #[tokio::test(flavor = "current_thread")]
    async fn a_request_waits_for_a_held_permit_and_gives_up_at_its_deadline() {
        let permits = Permits::new("permit_test_wait", 1);
        let held = permits.acquire("get", DEADLINE).await.unwrap();

        assert_eq!(
            permits_in_use()
                .with_label_values(&["permit_test_wait"])
                .get(),
            1
        );

        let error = match permits.acquire("get", DEADLINE).await {
            Ok(_) => panic!("the only permit is held"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("concurrency_wait"));
        assert_eq!(
            errors_total()
                .with_label_values(&["permit_test_wait", "get", "permit_wait"])
                .get(),
            1
        );
        assert_eq!(
            permits_waiting()
                .with_label_values(&["permit_test_wait"])
                .get(),
            0
        );

        drop(held);

        assert_eq!(
            permits_in_use()
                .with_label_values(&["permit_test_wait"])
                .get(),
            0
        );

        let waits = permit_wait().with_label_values(&["permit_test_wait", "get"]);
        assert_eq!(waits.get_sample_count(), 2);
        assert!(waits.get_sample_sum() >= DEADLINE.as_secs_f64());

        let holds = permit_hold().with_label_values(&["permit_test_wait", "get"]);
        assert_eq!(holds.get_sample_count(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_cancelled_wait_leaves_no_waiter_behind() {
        let permits = Permits::new("permit_test_cancel", 1);
        let _held = permits.acquire("put", DEADLINE).await.unwrap();

        let waiting = tokio::time::timeout(DEADLINE / 4, permits.acquire("put", DEADLINE)).await;

        assert!(waiting.is_err());
        assert_eq!(
            permits_waiting()
                .with_label_values(&["permit_test_cancel"])
                .get(),
            0
        );
    }
}
