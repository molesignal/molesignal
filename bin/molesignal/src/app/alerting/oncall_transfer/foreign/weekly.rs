// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 两家的时段限制可以跨天、跨周，也可以互相重叠；这里按分钟铺开，再写回轮值的每周时段。

use std::collections::BTreeMap;

use crate::domain::alerting::time_window::{ClockTime, EVERY_DAY, MINUTES_PER_DAY, WeeklyRange};

const DAY: usize = MINUTES_PER_DAY as usize;
pub(super) const WEEK: usize = 7 * DAY;

/// 一周里占了哪些分钟，从周日零点数起（本地时间）。
#[derive(Clone)]
pub(super) struct WeekMinutes(Vec<bool>);

impl WeekMinutes {
    pub fn empty() -> Self {
        Self(vec![false; WEEK])
    }

    pub fn full() -> Self {
        Self(vec![true; WEEK])
    }

    /// 从周日零点数起第 `start` 分钟开始，连续 `length` 分钟；过了周六接回周日。
    pub fn add(&mut self, start: usize, length: usize) {
        for offset in 0..length.min(WEEK) {
            self.0[(start + offset) % WEEK] = true;
        }
    }

    /// 每天都有的一段。
    pub fn add_daily(&mut self, start: usize, length: usize) {
        for day in 0..7 {
            self.add(day * DAY + start, length);
        }
    }

    pub fn is_empty(&self) -> bool {
        !self.0.iter().any(|on| *on)
    }

    pub fn intersects(&self, other: &Self) -> bool {
        self.0.iter().zip(&other.0).any(|(a, b)| *a && *b)
    }

    /// 写成每周时段：同样起止的几天合成一段，晚上接第二天早上的写成一段跨午夜的。整周都占时没有
    /// 时段（轮值的「一直生效」）；一分钟都没占时调用方先用 [`Self::is_empty`] 判断。
    pub fn ranges(&self) -> Vec<WeeklyRange> {
        if self.0.iter().all(|on| *on) {
            return Vec::new();
        }

        let mut spans = self.spans_by_day();

        join_across_midnight(&mut spans);
        spans
            .into_iter()
            .filter(|(_, mask)| *mask != 0)
            .filter_map(|((start, end), weekday_mask)| {
                Some(WeeklyRange {
                    weekday_mask,
                    start: clock(start)?,
                    end: clock(end)?,
                })
            })
            .collect()
    }

    /// 每天连续的几段：起止 → 哪几天。
    fn spans_by_day(&self) -> BTreeMap<(usize, usize), u8> {
        let mut spans: BTreeMap<(usize, usize), u8> = BTreeMap::new();

        for day in 0..7 {
            let minutes = &self.0[day * DAY..(day + 1) * DAY];
            let mut opened = None;

            for minute in 0..=DAY {
                match (minutes.get(minute).copied().unwrap_or(false), opened) {
                    (true, None) => opened = Some(minute),
                    (false, Some(start)) => {
                        *spans.entry((start, minute)).or_default() |= 1 << day;
                        opened = None;
                    }
                    _ => {}
                }
            }
        }

        spans
    }
}

fn clock(minutes: usize) -> Option<ClockTime> {
    ClockTime::new((minutes / 60) as u8, (minutes % 60) as u8)
}

/// 每一天的下一天。
fn next_days(mask: u8) -> u8 {
    ((mask << 1) | (mask >> 6)) & EVERY_DAY
}

/// 每一天的前一天。
fn previous_days(mask: u8) -> u8 {
    ((mask >> 1) | (mask << 6)) & EVERY_DAY
}

fn join_across_midnight(spans: &mut BTreeMap<(usize, usize), u8>) {
    let evenings: Vec<usize> = spans
        .keys()
        .filter(|(start, end)| *end == DAY && *start > 0)
        .map(|(start, _)| *start)
        .collect();
    let mornings: Vec<usize> = spans
        .keys()
        .filter(|(start, end)| *start == 0 && *end < DAY)
        .map(|(_, end)| *end)
        .collect();

    for start in &evenings {
        // 跨午夜的时段结束不能晚于开始：正好一整天的留着写成两段。
        for end in mornings.iter().filter(|end| *end < start) {
            let evening = spans[&(*start, DAY)];
            let morning = spans[&(0, *end)];
            let joined = evening & previous_days(morning);

            if joined == 0 {
                continue;
            }

            *spans.entry((*start, *end)).or_default() |= joined;
            spans.insert((*start, DAY), evening & !joined);
            spans.insert((0, *end), morning & !next_days(joined));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MONDAY: usize = DAY;
    const FRIDAY: usize = 5 * DAY;

    fn texts(minutes: &WeekMinutes) -> Vec<(u8, String, String)> {
        minutes
            .ranges()
            .into_iter()
            .map(|range| {
                (
                    range.weekday_mask,
                    range.start.to_string(),
                    range.end.to_string(),
                )
            })
            .collect()
    }

    /// 写出来的时段和铺开的分钟逐分钟一样。
    fn assert_same_week(minutes: &WeekMinutes) {
        let ranges = minutes.ranges();

        for day in 0..7u8 {
            for minute in 0..MINUTES_PER_DAY {
                let at = usize::from(day) * DAY + usize::from(minute);
                let written = ranges.is_empty()
                    || ranges.iter().any(|range| range.contains_local(day, minute));

                assert_eq!(written, minutes.0[at], "day {day} minute {minute}");
            }
        }

        for range in &ranges {
            range.validate().unwrap();
        }
    }

    #[test]
    fn a_daily_stretch_past_midnight_stays_one_range() {
        let mut minutes = WeekMinutes::empty();

        minutes.add_daily(18 * 60, 15 * 60);

        assert_eq!(
            texts(&minutes),
            vec![(EVERY_DAY, "18:00".into(), "09:00".into())]
        );
        assert_same_week(&minutes);
    }

    #[test]
    fn a_stretch_of_several_days_splits_into_first_middle_and_last() {
        let mut minutes = WeekMinutes::empty();

        // Monday 09:00 to Friday 17:00.
        minutes.add(MONDAY + 9 * 60, 4 * DAY + 8 * 60);

        assert_eq!(
            texts(&minutes),
            vec![
                (0b10_0000, "00:00".into(), "17:00".into()),
                (0b1_1100, "00:00".into(), "24:00".into()),
                (0b10, "09:00".into(), "24:00".into()),
            ]
        );
        assert_same_week(&minutes);
    }

    #[test]
    fn a_stretch_over_the_weekend_wraps_around_the_week() {
        let mut minutes = WeekMinutes::empty();

        // Friday 18:00 to Monday 08:00.
        minutes.add(FRIDAY + 18 * 60, 2 * DAY + 14 * 60);

        assert_same_week(&minutes);
        assert!(minutes.0[FRIDAY + 18 * 60]);
        assert!(minutes.0[MONDAY + 8 * 60 - 1]);
        assert!(!minutes.0[MONDAY + 8 * 60]);
    }

    #[test]
    fn exactly_a_day_from_the_afternoon_is_written_in_two_ranges() {
        let mut minutes = WeekMinutes::empty();

        minutes.add(MONDAY + 15 * 60, DAY);

        assert_eq!(minutes.ranges().len(), 2);
        assert_same_week(&minutes);
    }

    #[test]
    fn overlapping_stretches_merge_and_a_whole_week_needs_no_ranges() {
        let mut minutes = WeekMinutes::empty();

        minutes.add(MONDAY + 9 * 60, 8 * 60);
        minutes.add(MONDAY + 12 * 60, 8 * 60);

        assert_eq!(
            texts(&minutes),
            vec![(0b10, "09:00".into(), "20:00".into())]
        );
        assert_same_week(&minutes);

        minutes.add(0, WEEK);

        assert!(minutes.ranges().is_empty());
        assert!(!minutes.is_empty());
        assert!(WeekMinutes::empty().is_empty());
    }
}
