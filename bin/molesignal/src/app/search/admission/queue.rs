// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一个工作组在本节点的槽位与等待队列。槽位满时查询按到达顺序排队，空出的槽位交给等得最久
//! 的那条，新来的查询不会越过排队的查询。上限可随时调整：调高时立刻放行排在前面的查询，调低时
//! 已在执行的照常执行，降到新上限以下才放行。

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use parking_lot::Mutex;
use prometheus::IntGauge;
use tokio::sync::Notify;

/// 排队拿槽位的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Turn {
    /// 拿到了槽位，用完要 [`Slots::give_back`]。
    Admitted,
    /// 满了，且不排队。
    Full,
    /// 等满了时长仍没轮到。
    TimedOut,
}

/// 某一时刻的上限、执行数与排队数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Counts {
    pub(super) limit: usize,
    pub(super) running: usize,
    pub(super) waiting: usize,
}

/// 一个工作组的槽位。执行数与排队数同时写进它的两个 gauge。
pub(super) struct Slots {
    state: Mutex<State>,
    running: IntGauge,
    waiting: IntGauge,
}

struct State {
    /// 同时执行的上限；0 = 不限。
    limit: usize,
    running: usize,
    queue: VecDeque<Arc<Waiter>>,
}

#[derive(Default)]
struct Waiter {
    /// 槽位已交给它：在锁内置位，`running` 已算上它。
    granted: AtomicBool,
    notify: Notify,
}

impl Slots {
    pub(super) fn new(limit: usize, running: IntGauge, waiting: IntGauge) -> Self {
        Self {
            state: Mutex::new(State {
                limit,
                running: 0,
                queue: VecDeque::new(),
            }),
            running,
            waiting,
        }
    }

    pub(super) fn counts(&self) -> Counts {
        let state = self.state.lock();

        Counts {
            limit: state.limit,
            running: state.running,
            waiting: state.queue.len(),
        }
    }

    pub(super) fn set_limit(&self, limit: usize) {
        let mut state = self.state.lock();
        state.limit = limit;
        state.admit_waiting();
        self.publish(&state);
    }

    /// 拿一个槽位。有空位且没人排队时立刻拿到；否则排队，至多等 `wait`，`wait` 为零时满了就
    /// 不排。等待途中被丢弃（请求断开）时退出队列，已经交到手的槽位还回去。
    pub(super) async fn take(&self, wait: Duration) -> Turn {
        let waiter = {
            let mut state = self.state.lock();

            if state.has_room() && state.queue.is_empty() {
                state.running += 1;
                self.publish(&state);
                return Turn::Admitted;
            }

            if wait.is_zero() {
                return Turn::Full;
            }

            let waiter = Arc::new(Waiter::default());
            state.queue.push_back(waiter.clone());
            self.publish(&state);
            waiter
        };

        let queued = Queued {
            slots: self,
            waiter: Some(waiter.clone()),
        };

        let notified = tokio::time::timeout(wait, waiter.notify.notified()).await;
        queued.settle();

        if notified.is_ok() {
            return Turn::Admitted;
        }

        // 到点的同时，槽位可能刚好交了过来。
        let mut state = self.state.lock();

        if waiter.granted.load(Ordering::Relaxed) {
            return Turn::Admitted;
        }

        state.leave(&waiter);
        self.publish(&state);
        Turn::TimedOut
    }

    /// 还一个槽位：交给排在最前面的查询，没人排队就空出来。
    pub(super) fn give_back(&self) {
        let mut state = self.state.lock();
        state.running -= 1;
        state.admit_waiting();
        self.publish(&state);
    }

    fn publish(&self, state: &State) {
        self.running.set(state.running as i64);
        self.waiting.set(state.queue.len() as i64);
    }
}

impl State {
    fn has_room(&self) -> bool {
        self.limit == 0 || self.running < self.limit
    }

    /// 有空位就按排队顺序放行。
    fn admit_waiting(&mut self) {
        while self.has_room() {
            let Some(waiter) = self.queue.pop_front() else {
                break;
            };

            waiter.granted.store(true, Ordering::Relaxed);
            waiter.notify.notify_one();
            self.running += 1;
        }
    }

    fn leave(&mut self, waiter: &Arc<Waiter>) {
        self.queue.retain(|queued| !Arc::ptr_eq(queued, waiter));
    }
}

/// 一条正在排队的查询。等待途中被丢弃时退出队列；槽位若已交到它手上，就还回去。
struct Queued<'a> {
    slots: &'a Slots,
    waiter: Option<Arc<Waiter>>,
}

impl Queued<'_> {
    /// 等待有了结果，由 [`Slots::take`] 自己收尾。
    fn settle(mut self) {
        self.waiter = None;
    }
}

impl Drop for Queued<'_> {
    fn drop(&mut self) {
        let Some(waiter) = self.waiter.take() else {
            return;
        };

        let mut state = self.slots.state.lock();

        if waiter.granted.load(Ordering::Relaxed) {
            state.running -= 1;
            state.admit_waiting();
        } else {
            state.leave(&waiter);
        }

        self.slots.publish(&state);
    }
}

#[cfg(test)]
mod tests {
    use tokio::task::JoinHandle;

    use super::*;

    const WAIT: Duration = Duration::from_secs(10);

    fn slots(limit: usize) -> Arc<Slots> {
        Arc::new(Slots::new(
            limit,
            IntGauge::new("test_running", "running").unwrap(),
            IntGauge::new("test_waiting", "waiting").unwrap(),
        ))
    }

    fn counts(slots: &Slots) -> (usize, usize) {
        let counts = slots.counts();
        (counts.running, counts.waiting)
    }

    /// 一条拿不到槽位的查询，排进队列后才返回。
    async fn queue_up(slots: &Arc<Slots>) -> JoinHandle<Turn> {
        let before = slots.counts().waiting;

        let task = tokio::spawn({
            let slots = slots.clone();
            async move { slots.take(WAIT).await }
        });

        while slots.counts().waiting == before {
            tokio::task::yield_now().await;
        }

        task
    }

    #[tokio::test]
    async fn free_slots_are_taken_at_once_and_a_full_group_without_a_wait_refuses() {
        let slots = slots(2);

        assert_eq!(slots.take(WAIT).await, Turn::Admitted);
        assert_eq!(slots.take(Duration::ZERO).await, Turn::Admitted);
        assert_eq!(slots.take(Duration::ZERO).await, Turn::Full);
        assert_eq!(counts(&slots), (2, 0));
    }

    #[tokio::test]
    async fn the_query_that_waited_longest_takes_the_freed_slot() {
        let slots = slots(1);
        assert_eq!(slots.take(WAIT).await, Turn::Admitted);

        let first = queue_up(&slots).await;
        let second = queue_up(&slots).await;

        assert_eq!(counts(&slots), (1, 2));
        assert_eq!((slots.running.get(), slots.waiting.get()), (1, 2));

        slots.give_back();

        assert_eq!(first.await.unwrap(), Turn::Admitted);
        assert_eq!(counts(&slots), (1, 1));

        slots.give_back();

        assert_eq!(second.await.unwrap(), Turn::Admitted);
        assert_eq!(counts(&slots), (1, 0));
        assert_eq!((slots.running.get(), slots.waiting.get()), (1, 0));
    }

    #[tokio::test]
    async fn a_query_that_waits_too_long_is_refused_and_leaves_the_queue() {
        let slots = slots(1);
        assert_eq!(slots.take(WAIT).await, Turn::Admitted);

        assert_eq!(slots.take(Duration::from_millis(20)).await, Turn::TimedOut);
        assert_eq!(counts(&slots), (1, 0));

        slots.give_back();
        assert_eq!(counts(&slots), (0, 0));
    }

    #[tokio::test]
    async fn a_query_dropped_while_waiting_leaves_the_queue() {
        let slots = slots(1);
        assert_eq!(slots.take(WAIT).await, Turn::Admitted);

        let waiting = queue_up(&slots).await;
        waiting.abort();
        let _ = waiting.await;

        assert_eq!(counts(&slots), (1, 0));

        slots.give_back();
        assert_eq!(counts(&slots), (0, 0));
    }

    #[tokio::test]
    async fn a_slot_handed_to_a_query_dropped_before_it_woke_goes_back() {
        let slots = slots(1);
        assert_eq!(slots.take(WAIT).await, Turn::Admitted);

        let mut waiting = Box::pin(slots.take(WAIT));
        assert!(futures::poll!(waiting.as_mut()).is_pending());

        // 槽位交给了排队的查询，而它不会再被轮询。
        slots.give_back();
        assert_eq!(counts(&slots), (1, 0));

        drop(waiting);
        assert_eq!(counts(&slots), (0, 0));
    }

    #[tokio::test]
    async fn raising_the_limit_lets_waiting_queries_in() {
        let slots = slots(1);
        assert_eq!(slots.take(WAIT).await, Turn::Admitted);

        let first = queue_up(&slots).await;
        let second = queue_up(&slots).await;
        slots.set_limit(3);

        assert_eq!(first.await.unwrap(), Turn::Admitted);
        assert_eq!(second.await.unwrap(), Turn::Admitted);
        assert_eq!(counts(&slots), (3, 0));
    }

    #[tokio::test]
    async fn lowering_the_limit_lets_running_queries_finish_first() {
        let slots = slots(2);

        for _ in 0..2 {
            assert_eq!(slots.take(WAIT).await, Turn::Admitted);
        }

        slots.set_limit(1);
        let waiting = queue_up(&slots).await;

        slots.give_back();
        assert_eq!(counts(&slots), (1, 1));

        slots.give_back();
        assert_eq!(waiting.await.unwrap(), Turn::Admitted);
        assert_eq!(counts(&slots), (1, 0));
    }

    #[tokio::test]
    async fn without_a_limit_nothing_waits() {
        let slots = slots(0);

        for _ in 0..100 {
            assert_eq!(slots.take(Duration::ZERO).await, Turn::Admitted);
        }

        assert_eq!(counts(&slots), (100, 0));
    }
}
