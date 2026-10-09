// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 滑动窗口计数：按秒分桶，只保留窗口内的桶。

use std::collections::VecDeque;

#[derive(Debug, Default)]
pub(super) struct SlidingCounter {
    /// `(秒, 该秒内的命中数)`，秒单调递增。
    buckets: VecDeque<(i64, u64)>,
    total: u64,
}

impl SlidingCounter {
    pub(super) fn add(&mut self, second: i64, count: u64) {
        match self.buckets.back_mut() {
            // 同一秒，或时钟回拨：并入最后一个桶，桶数仍受窗口长度约束。
            Some((last, bucket)) if *last >= second => *bucket += count,
            _ => self.buckets.push_back((second, count)),
        }
        self.total += count;
    }

    /// 丢弃 `(second - window_secs, second]` 之外的桶，返回窗口内的命中总数。
    pub(super) fn count(&mut self, second: i64, window_secs: u32) -> u64 {
        let oldest_excluded = second - i64::from(window_secs);
        while let Some(&(bucket_second, bucket)) = self.buckets.front() {
            if bucket_second > oldest_excluded {
                break;
            }
            self.total -= bucket;
            self.buckets.pop_front();
        }
        self.total
    }
}

#[cfg(test)]
mod tests {
    use super::SlidingCounter;

    #[test]
    fn counts_only_the_seconds_inside_the_window() {
        let mut counter = SlidingCounter::default();
        counter.add(100, 3);
        counter.add(101, 2);
        counter.add(105, 4);

        // 窗口 10 秒：在 105 秒覆盖 96..=105，三个桶都在内。
        assert_eq!(counter.count(105, 10), 9);
        // 在 110 秒覆盖 101..=110：第 100 秒的桶离开窗口。
        assert_eq!(counter.count(110, 10), 6);
        // 在 111 秒覆盖 102..=111：第 101 秒的桶也离开。
        assert_eq!(counter.count(111, 10), 4);
        assert_eq!(counter.count(115, 10), 0);
    }

    #[test]
    fn events_in_the_same_second_share_a_bucket() {
        let mut counter = SlidingCounter::default();
        for _ in 0..1000 {
            counter.add(7, 1);
        }
        assert_eq!(counter.buckets.len(), 1);
        assert_eq!(counter.count(7, 1), 1000);
        assert_eq!(counter.count(8, 1), 0);
    }

    #[test]
    fn a_backwards_clock_does_not_grow_the_window() {
        let mut counter = SlidingCounter::default();
        counter.add(50, 1);
        counter.add(49, 1);
        assert_eq!(counter.buckets.len(), 1);
        assert_eq!(counter.count(50, 5), 2);
    }
}
