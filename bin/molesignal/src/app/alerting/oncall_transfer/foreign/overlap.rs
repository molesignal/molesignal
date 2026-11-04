// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Opsgenie 的轮换和 PagerDuty 的 shift-based 事件之间没有优先级，时间上重叠就是几个人同时值班。一份排班同一时刻
//! 只有一个值班人，所以重叠的挡住写入。

use chrono_tz::Tz;

use super::{Draft, local_text, weekly::WeekMinutes};
use crate::{
    app::alerting::oncall_transfer::Finding, domain::alerting::schedule::Rotation,
    shared::time::TimestampMicros,
};

const MINUTE_MICROS: i64 = 60_000_000;
/// 生效时段按周重复：多看一天，一周里没碰上就不会碰上。
const OVERLAP_HORIZON_MICROS: i64 = 8 * 1440 * MINUTE_MICROS;

/// 两条轮换会同时有人值班就挡住写入，说出第一次碰上的时刻。`weeks` 和 `draft.rotations` 一一对应。
pub(super) fn check_overlaps(draft: &mut Draft<'_>, weeks: &[WeekMinutes], tz: Tz, now: i64) {
    let mut found = Vec::new();

    for (index, first) in draft.rotations.iter().enumerate() {
        for (offset, second) in draft.rotations[index + 1..].iter().enumerate() {
            if !weeks[index].intersects(&weeks[index + 1 + offset]) {
                continue;
            }

            if let Some(at) = first_overlap(first, second, tz, now) {
                found.push(
                    Finding::new("rotations_overlap")
                        .with("rotation", first.name.clone())
                        .with("other", second.name.clone())
                        .with("at", local_text(at, tz)),
                );
            }
        }
    }

    draft.blockers.extend(found);
}

/// 两条轮换第一次同时生效的时刻：从现在（或后开始的那条开始）起，按分钟走。
fn first_overlap(first: &Rotation, second: &Rotation, tz: Tz, now: i64) -> Option<i64> {
    let from = now.max(first.start_at.0).max(second.start_at.0);
    let from = from + (MINUTE_MICROS - from.rem_euclid(MINUTE_MICROS)) % MINUTE_MICROS;
    let to = [first.end_at, second.end_at]
        .into_iter()
        .flatten()
        .map(|end| end.0)
        .chain([from + OVERLAP_HORIZON_MICROS])
        .min()
        .unwrap_or(from);

    (0..)
        .map(|step| from + step * MINUTE_MICROS)
        .take_while(|at| *at < to)
        .find(|at| {
            let at = TimestampMicros(*at);

            first.resolve_in_tz(at, tz).is_some() && second.resolve_in_tz(at, tz).is_some()
        })
}
