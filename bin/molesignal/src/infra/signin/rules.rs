// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 密码登录次数的规则，和存储无关。
//!
//! 一次尝试涉及三行计数：账号，来源，账号 + 来源。没有来源时只有账号这一行。
//!
//! - 来源和「账号 + 来源」到限就锁一段时间，只挡这个来源。
//! - 账号从不锁：到限之后每隔一段时间才放一次，最近在这个来源成功过的不受限，所以别人输错密码
//!   挡不住本人。
//!
//! 核对密码之前先把次数记上，对了再退回，否则并发的请求能在第一个失败记下之前全部通过检查。被拒的
//! 请求不核对密码，也不延长任何锁，所以锁最长就是设定的时长。

use crate::{domain::iam::SigninLimits, shared::time::TimestampMicros};

/// 账号限速不管这么久之内成功过的来源。
pub(super) const KNOWN_SOURCE_MICROS: i64 = 30 * 24 * 60 * 60 * 1_000_000;
/// 一次尝试的账号和来源，都是摘要。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptKeys {
    pub account: String,
    pub source: Option<String>,
}

/// 一行的主键：只按一个维度计时另一个是空串。
pub(super) type RowKey = (String, String);

impl AttemptKeys {
    pub(super) fn account_row(&self) -> RowKey {
        (self.account.clone(), String::new())
    }

    pub(super) fn source_row(&self) -> Option<RowKey> {
        self.source
            .as_ref()
            .map(|source| (String::new(), source.clone()))
    }

    pub(super) fn pair_row(&self) -> Option<RowKey> {
        self.source
            .as_ref()
            .map(|source| (self.account.clone(), source.clone()))
    }

    /// 这次尝试涉及的行，按主键排序：所有事务按同一顺序上锁，不会互相死锁。
    pub(super) fn rows(&self) -> Vec<RowKey> {
        let mut rows = vec![self.account_row()];
        rows.extend(self.source_row());
        rows.extend(self.pair_row());
        rows.sort();
        rows
    }
}

/// 一行计数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Tally {
    pub(super) failures: i32,
    pub(super) window_started_at: TimestampMicros,
    pub(super) locked_until: Option<TimestampMicros>,
    pub(super) last_success_at: Option<TimestampMicros>,
}

/// 一次尝试涉及的三行。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Tallies {
    pub(super) account: Option<Tally>,
    pub(super) source: Option<Tally>,
    pub(super) pair: Option<Tally>,
}

/// 设置页上的参数，换算成计算要用的形式。次数为 0 时关掉这一项。
#[derive(Debug, Clone, Copy)]
pub(super) struct Rules {
    pair_limit: i32,
    source_limit: i32,
    account_limit: i32,
    window_micros: i64,
    lock_micros: i64,
    interval_micros: i64,
}

impl From<&SigninLimits> for Rules {
    fn from(limits: &SigninLimits) -> Self {
        let lock_micros = i64::from(limits.lock_minutes) * 60_000_000;

        Self {
            pair_limit: i32::try_from(limits.account_source_failures).unwrap_or(i32::MAX),
            source_limit: i32::try_from(limits.source_failures).unwrap_or(i32::MAX),
            account_limit: i32::try_from(limits.account_failures).unwrap_or(i32::MAX),
            window_micros: lock_micros,
            lock_micros,
            interval_micros: i64::from(limits.account_interval_secs) * 1_000_000,
        }
    }
}

/// 拒绝一次尝试。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refusal {
    pub until: TimestampMicros,
    /// 只是账号限速，没有来源锁。
    pub paced: bool,
}

/// 这次尝试让哪几行刚好到限。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tripped {
    pub account: bool,
    pub source: bool,
    pub pair: bool,
}

/// 记上一次尝试的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Begun {
    pub refusal: Option<Refusal>,
    pub tripped: Tripped,
}

/// 这次尝试为什么不算失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settlement {
    /// 密码是对的。
    Succeeded,
    /// 没核对出结果，例如目录服务连不上。
    Inconclusive,
}

/// 发信限额：一个时间窗里最多几次，用掉不退。
#[derive(Debug, Clone, Copy)]
pub struct Quota {
    pub per_account: i32,
    pub per_source: i32,
    pub window_micros: i64,
}

fn locked(tally: Option<Tally>, now: TimestampMicros) -> Option<TimestampMicros> {
    tally?.locked_until.filter(|until| *until > now)
}

fn known(pair: Option<Tally>, now: TimestampMicros) -> bool {
    pair.and_then(|tally| tally.last_success_at)
        .is_some_and(|at| now.0 - at.0 < KNOWN_SOURCE_MICROS)
}

/// 现在拦不拦这次尝试。来源锁优先于账号限速。
pub(super) fn refusal(tallies: &Tallies, rules: &Rules, now: TimestampMicros) -> Option<Refusal> {
    let source = locked(tallies.source, now).filter(|_| rules.source_limit > 0);
    let pair = locked(tallies.pair, now).filter(|_| rules.pair_limit > 0);

    let paced = locked(tallies.account, now)
        .filter(|_| rules.account_limit > 0 && !known(tallies.pair, now));

    match source.max(pair) {
        Some(until) => Some(Refusal {
            until: paced.map_or(until, |paced| paced.max(until)),
            paced: false,
        }),
        None => paced.map(|until| Refusal { until, paced: true }),
    }
}

/// 来源和「账号 + 来源」再记一次：时间窗过了或者上一次锁过了就重新开始，到限就锁。
fn counted(previous: Option<Tally>, limit: i32, rules: &Rules, now: TimestampMicros) -> Tally {
    let open = previous.filter(|tally| {
        tally.window_started_at.0 > now.0 - rules.window_micros
            && tally.locked_until.is_none_or(|until| until > now)
    });

    let failures = open.map_or(0, |tally| tally.failures).saturating_add(1);

    Tally {
        failures,
        window_started_at: open.map_or(now, |tally| tally.window_started_at),
        locked_until: (failures >= limit).then_some(TimestampMicros(now.0 + rules.lock_micros)),
        last_success_at: previous.and_then(|tally| tally.last_success_at),
    }
}

/// 账号再记一次：连续一个时间窗没有新失败才清零，所以时间窗每次都从现在算起。到限之后，每记一次
/// 就要隔一段时间才放下一次。
fn paced(previous: Option<Tally>, rules: &Rules, now: TimestampMicros) -> Tally {
    let open = previous.filter(|tally| tally.window_started_at.0 > now.0 - rules.window_micros);
    let failures = open.map_or(0, |tally| tally.failures).saturating_add(1);

    Tally {
        failures,
        window_started_at: now,
        locked_until: (failures >= rules.account_limit)
            .then_some(TimestampMicros(now.0 + rules.interval_micros)),
        last_success_at: None,
    }
}

/// 这次尝试之后的三行，和这次尝试的结果。
pub(super) fn begin(
    tallies: Tallies,
    has_source: bool,
    rules: &Rules,
    now: TimestampMicros,
) -> (Tallies, Begun) {
    if let Some(refusal) = refusal(&tallies, rules, now) {
        let mut next = tallies;
        let mut tripped = Tripped::default();

        // 抢账号限速的名额要付代价：计入来源次数。本人照提示等着，不会很快锁住自己。
        if refusal.paced && has_source && rules.source_limit > 0 {
            let source = counted(tallies.source, rules.source_limit, rules, now);
            tripped.source = source.failures == rules.source_limit;
            next.source = Some(source);
        }

        return (
            next,
            Begun {
                refusal: Some(refusal),
                tripped,
            },
        );
    }

    let mut next = tallies;
    let mut tripped = Tripped::default();

    if rules.account_limit > 0 {
        let account = paced(tallies.account, rules, now);
        tripped.account = account.failures == rules.account_limit;
        next.account = Some(account);
    }

    if has_source && rules.source_limit > 0 {
        let source = counted(tallies.source, rules.source_limit, rules, now);
        tripped.source = source.failures == rules.source_limit;
        next.source = Some(source);
    }

    if has_source && rules.pair_limit > 0 {
        let pair = counted(tallies.pair, rules.pair_limit, rules, now);
        tripped.pair = pair.failures == rules.pair_limit;
        next.pair = Some(pair);
    }

    (
        next,
        Begun {
            refusal: None,
            tripped,
        },
    )
}

/// 退回记上的一次；退到限以下时，这次尝试加上的锁也解开。
fn refunded(tally: Option<Tally>, limit: i32) -> Option<Tally> {
    tally.map(|tally| {
        let failures = tally.failures.saturating_sub(1).max(0);

        Tally {
            failures,
            locked_until: tally.locked_until.filter(|_| failures >= limit),
            ..tally
        }
    })
}

/// 尝试有了结果、不算失败之后的三行。来源和账号的计数只退回这一次，不清零：否则攻击者登录自己的
/// 账号就能把来源计数清零。
pub(super) fn settled(
    tallies: Tallies,
    has_source: bool,
    settlement: Settlement,
    rules: &Rules,
    now: TimestampMicros,
) -> Tallies {
    let pair = match settlement {
        Settlement::Succeeded if has_source => Some(succeeded_pair(now)),
        Settlement::Succeeded => tallies.pair,
        Settlement::Inconclusive => refunded(tallies.pair, rules.pair_limit),
    };

    Tallies {
        account: refunded(tallies.account, rules.account_limit),
        source: refunded(tallies.source, rules.source_limit),
        pair,
    }
}

pub(super) fn succeeded_pair(now: TimestampMicros) -> Tally {
    Tally {
        failures: 0,
        window_started_at: now,
        locked_until: None,
        last_success_at: Some(now),
    }
}

/// 占一次限额之后的计数；用完了回答 `None`。
pub(super) fn quota_taken(
    previous: Option<Tally>,
    limit: i32,
    window_micros: i64,
    now: TimestampMicros,
) -> Option<Tally> {
    let open = previous.filter(|tally| tally.window_started_at.0 > now.0 - window_micros);
    let used = open.map_or(0, |tally| tally.failures);

    (used < limit).then(|| Tally {
        failures: used + 1,
        window_started_at: open.map_or(now, |tally| tally.window_started_at),
        locked_until: None,
        last_success_at: None,
    })
}

/// 要写回的一行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Written {
    pub(super) key: RowKey,
    pub(super) tally: Tally,
    pub(super) expires_at: TimestampMicros,
}

/// 三行里变了的那些，带上它们什么时候可以删。
pub(super) fn written(
    keys: &AttemptKeys,
    before: &Tallies,
    after: &Tallies,
    rules: &Rules,
    now: TimestampMicros,
) -> Vec<Written> {
    let source_expires = TimestampMicros(now.0 + rules.window_micros + rules.lock_micros);

    let mut rows = Vec::new();

    if let Some(tally) = after.account.filter(|_| before.account != after.account) {
        rows.push(Written {
            key: keys.account_row(),
            tally,
            expires_at: TimestampMicros(now.0 + rules.window_micros + rules.interval_micros),
        });
    }

    if let Some((key, tally)) = keys
        .source_row()
        .zip(after.source.filter(|_| before.source != after.source))
    {
        rows.push(Written {
            key,
            tally,
            expires_at: source_expires,
        });
    }

    if let Some((key, tally)) = keys
        .pair_row()
        .zip(after.pair.filter(|_| before.pair != after.pair))
    {
        let known_until = tally
            .last_success_at
            .map_or(now.0, |at| at.0 + KNOWN_SOURCE_MICROS);

        rows.push(Written {
            key,
            tally,
            expires_at: TimestampMicros(source_expires.0.max(known_until)),
        });
    }

    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: i64 = 60_000_000;

    fn limits() -> SigninLimits {
        SigninLimits {
            account_source_failures: 3,
            source_failures: 5,
            account_failures: 4,
            account_interval_secs: 60,
            lock_minutes: 15,
        }
    }

    fn keys() -> AttemptKeys {
        AttemptKeys {
            account: "account".into(),
            source: Some("source".into()),
        }
    }

    fn at(minutes: i64) -> TimestampMicros {
        TimestampMicros(1_000_000_000_000 + minutes * MINUTE)
    }

    /// 一次次尝试的存储：除了 SQL，和真实的走同样的计算。
    struct Store {
        rules: Rules,
        tallies: Tallies,
    }

    impl Store {
        fn new(limits: &SigninLimits) -> Self {
            Self {
                rules: Rules::from(limits),
                tallies: Tallies::default(),
            }
        }

        fn current(&self) -> Tallies {
            self.tallies
        }

        fn begin(&mut self, now: TimestampMicros) -> Begun {
            let (after, begun) = begin(self.tallies, true, &self.rules, now);
            self.tallies = after;
            begun
        }

        fn settle(&mut self, settlement: Settlement, now: TimestampMicros) {
            self.tallies = settled(self.tallies, true, settlement, &self.rules, now);
        }
    }

    #[test]
    fn the_limit_locks_only_after_that_many_failures() {
        let mut store = Store::new(&limits());

        assert_eq!(store.begin(at(0)).refusal, None);
        assert_eq!(store.begin(at(1)).refusal, None);

        let third = store.begin(at(2));
        assert_eq!(third.refusal, None);
        assert!(third.tripped.pair);
        assert!(!third.tripped.source);

        let refused = store.begin(at(3)).refusal.unwrap();
        assert_eq!(refused.until, TimestampMicros(at(2).0 + 15 * MINUTE));
        assert!(!refused.paced);
    }

    #[test]
    fn a_refused_attempt_does_not_extend_a_lock() {
        let mut store = Store::new(&limits());

        for minute in 0..3 {
            store.begin(at(minute));
        }

        let first = store.begin(at(3)).refusal.unwrap();
        let later = store.begin(at(10)).refusal.unwrap();
        assert_eq!(first.until, later.until);

        // 锁过了就重新开始。
        assert_eq!(store.begin(at(17)).refusal, None);
        assert_eq!(store.current().pair.unwrap().failures, 1);
    }

    #[test]
    fn an_old_window_starts_over() {
        let mut store = Store::new(&limits());

        store.begin(at(0));
        store.begin(at(1));
        assert_eq!(store.begin(at(20)).refusal, None);
        assert_eq!(store.current().pair.unwrap().failures, 1);
        assert_eq!(store.current().pair.unwrap().window_started_at, at(20));
    }

    #[test]
    fn a_correct_password_clears_the_pair_but_only_refunds_the_rest() {
        let mut store = Store::new(&limits());

        store.begin(at(0));
        store.begin(at(1));
        store.begin(at(2));
        store.settle(Settlement::Succeeded, at(2));

        let current = store.current();
        let pair = current.pair.unwrap();

        assert_eq!(pair.failures, 0);
        assert_eq!(pair.locked_until, None);
        assert_eq!(pair.last_success_at, Some(at(2)));

        // 另外两行只退回最后一次。
        assert_eq!(current.source.unwrap().failures, 2);
        assert_eq!(current.account.unwrap().failures, 2);
    }

    #[test]
    fn signing_in_to_your_own_account_does_not_reset_the_source() {
        let mut limits = limits();
        limits.account_source_failures = 0;
        limits.account_failures = 0;

        let mut store = Store::new(&limits);

        // 猜别的账号四次，登录自己的账号一次。
        for minute in 0..4 {
            store.begin(at(minute));
        }

        store.begin(at(4));
        store.settle(Settlement::Succeeded, at(4));

        // 还差一次到限，不是从头开始。
        assert_eq!(store.current().source.unwrap().failures, 4);
        assert!(store.begin(at(5)).tripped.source);
    }

    #[test]
    fn the_attempt_that_reaches_the_limit_can_still_succeed() {
        let mut store = Store::new(&limits());

        store.begin(at(0));
        store.begin(at(0));
        store.begin(at(0));
        assert!(locked(store.current().pair, at(0)).is_some());

        store.settle(Settlement::Succeeded, at(0));
        assert_eq!(locked(store.current().pair, at(0)), None);
        assert_eq!(store.begin(at(0)).refusal, None);
    }

    #[test]
    fn an_unreachable_directory_does_not_count() {
        let mut store = Store::new(&limits());

        store.begin(at(0));
        store.settle(Settlement::Inconclusive, at(0));

        let current = store.current();

        assert_eq!(current.pair.unwrap().failures, 0);
        assert_eq!(current.pair.unwrap().last_success_at, None);
        assert_eq!(current.source.unwrap().failures, 0);
        assert_eq!(current.account.unwrap().failures, 0);
    }

    /// 别的来源输错很多次之后的账号这一行。
    fn account_at_the_limit(rules: &Rules, now: TimestampMicros) -> Option<Tally> {
        let mut account = None;

        for _ in 0..rules.account_limit {
            account = Some(paced(account, rules, now));
        }

        account
    }

    #[test]
    fn an_account_at_the_limit_is_paced_and_never_locked_for_long() {
        let rules = Rules::from(&limits());
        let now = at(0);

        let tallies = Tallies {
            account: account_at_the_limit(&rules, now),
            ..Tallies::default()
        };

        let refused = refusal(&tallies, &rules, now).unwrap();
        assert!(refused.paced);
        assert_eq!(refused.until, TimestampMicros(now.0 + MINUTE));

        // 一分钟之后放一次，这一次之后再等一分钟。
        let later = at(1);
        assert_eq!(refusal(&tallies, &rules, later), None);

        let (after, begun) = begin(tallies, true, &rules, later);
        assert_eq!(begun.refusal, None);
        assert!(!begun.tripped.account);
        assert!(refusal(&after, &rules, later).unwrap().paced);
    }

    #[test]
    fn the_account_counter_resets_only_after_a_quiet_window() {
        let rules = Rules::from(&limits());

        let first = paced(None, &rules, at(0));
        let second = paced(Some(first), &rules, at(14));
        assert_eq!(second.failures, 2);
        assert_eq!(second.window_started_at, at(14));

        let third = paced(Some(second), &rules, at(28));
        assert_eq!(third.failures, 3);

        let fresh = paced(Some(third), &rules, at(44));
        assert_eq!(fresh.failures, 1);
    }

    #[test]
    fn a_source_that_signed_in_recently_is_not_paced() {
        let rules = Rules::from(&limits());
        let now = at(0);

        let tallies = Tallies {
            account: account_at_the_limit(&rules, now),
            source: None,
            pair: Some(succeeded_pair(TimestampMicros(
                now.0 - 29 * 24 * 60 * MINUTE,
            ))),
        };

        assert_eq!(refusal(&tallies, &rules, now), None);

        let long_ago = Tallies {
            pair: Some(succeeded_pair(TimestampMicros(
                now.0 - 31 * 24 * 60 * MINUTE,
            ))),
            ..tallies
        };

        assert!(refusal(&long_ago, &rules, now).unwrap().paced);
    }

    #[test]
    fn a_known_source_is_still_stopped_by_its_own_failures() {
        let rules = Rules::from(&limits());
        let now = at(0);

        let mut pair = succeeded_pair(now);
        pair.locked_until = Some(TimestampMicros(now.0 + MINUTE));

        let tallies = Tallies {
            pair: Some(pair),
            ..Tallies::default()
        };

        assert!(!refusal(&tallies, &rules, now).unwrap().paced);
    }

    #[test]
    fn asking_for_a_paced_slot_costs_the_source() {
        let rules = Rules::from(&limits());
        let now = at(0);

        let mut tallies = Tallies {
            account: account_at_the_limit(&rules, now),
            ..Tallies::default()
        };

        let mut tripped = false;

        for _ in 0..rules.source_limit {
            let (after, begun) = begin(tallies, true, &rules, now);
            assert!(begun.refusal.unwrap().paced);
            tripped = begun.tripped.source;
            tallies = after;
        }

        assert!(tripped);

        // 记到限之后，来源锁住了。账号这一行和「账号 + 来源」都没有多记。
        assert!(!refusal(&tallies, &rules, now).unwrap().paced);
        assert_eq!(tallies.account.unwrap().failures, rules.account_limit);
        assert_eq!(tallies.pair, None);
    }

    #[test]
    fn with_no_source_only_the_account_counts() {
        let rules = Rules::from(&limits());
        let (after, begun) = begin(Tallies::default(), false, &rules, at(0));

        assert_eq!(begun.refusal, None);
        assert_eq!(after.account.unwrap().failures, 1);
        assert_eq!(after.source, None);
        assert_eq!(after.pair, None);
    }

    #[test]
    fn a_limit_of_zero_turns_that_count_off() {
        let limits = SigninLimits {
            account_source_failures: 0,
            source_failures: 0,
            account_failures: 0,
            ..limits()
        };

        let rules = Rules::from(&limits);
        let mut tallies = Tallies::default();

        for _ in 0..100 {
            let (after, begun) = begin(tallies, true, &rules, at(0));
            assert_eq!(begun.refusal, None);
            tallies = after;
        }

        assert_eq!(tallies, Tallies::default());

        // 关掉之后，已有的锁也不再挡人。
        let mut locked_tallies = Tallies {
            source: Some(counted(None, 1, &rules, at(0))),
            ..Tallies::default()
        };

        locked_tallies.pair = locked_tallies.source;
        assert_eq!(refusal(&locked_tallies, &rules, at(0)), None);
    }

    #[test]
    fn rows_expire_after_what_they_remember() {
        let rules = Rules::from(&limits());
        let keys = keys();
        let now = at(0);

        let (after, _) = begin(Tallies::default(), true, &rules, now);
        let rows = written(&keys, &Tallies::default(), &after, &rules, now);
        assert_eq!(rows.len(), 3);

        let pair = rows.iter().find(|row| row.key == keys.pair_row().unwrap());
        assert_eq!(
            pair.unwrap().expires_at,
            TimestampMicros(now.0 + 30 * MINUTE)
        );

        let account = rows.iter().find(|row| row.key == keys.account_row());
        assert_eq!(
            account.unwrap().expires_at,
            TimestampMicros(now.0 + 16 * MINUTE)
        );

        // 成功过的来源记 30 天。
        let succeeded = settled(after, true, Settlement::Succeeded, &rules, now);
        let rows = written(&keys, &after, &succeeded, &rules, now);
        let pair = rows.iter().find(|row| row.key == keys.pair_row().unwrap());

        assert_eq!(
            pair.unwrap().expires_at,
            TimestampMicros(now.0 + KNOWN_SOURCE_MICROS)
        );
    }

    #[test]
    fn rows_that_did_not_change_are_not_written() {
        let rules = Rules::from(&limits());
        let tallies = Tallies::default();

        assert!(written(&keys(), &tallies, &tallies, &rules, at(0)).is_empty());
    }

    #[test]
    fn the_quota_stops_at_the_limit_until_its_window_ends() {
        let window = 60 * MINUTE;
        let mut tally = None;

        for used in 1..=2 {
            tally = quota_taken(tally, 2, window, at(0));
            assert_eq!(tally.unwrap().failures, used);
        }

        assert_eq!(quota_taken(tally, 2, window, at(59)), None);

        let fresh = quota_taken(tally, 2, window, at(60)).unwrap();
        assert_eq!(fresh.failures, 1);
        assert_eq!(fresh.window_started_at, at(60));
    }

    #[test]
    fn keys_lock_rows_in_one_order() {
        let keys = AttemptKeys {
            account: "b".into(),
            source: Some("a".into()),
        };

        assert_eq!(
            keys.rows(),
            vec![
                (String::new(), "a".to_owned()),
                ("b".to_owned(), String::new()),
                ("b".to_owned(), "a".to_owned()),
            ]
        );

        let no_source = AttemptKeys {
            account: "b".into(),
            source: None,
        };

        assert_eq!(no_source.rows(), vec![("b".to_owned(), String::new())]);
    }
}
