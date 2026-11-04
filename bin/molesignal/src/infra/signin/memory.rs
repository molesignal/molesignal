// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 单测用的次数存储：除了 SQL，和 [`super::PgSigninAttemptRepository`] 走同样的计算。

use std::{collections::HashMap, sync::Mutex};

use async_trait::async_trait;

use super::{
    attempts::SigninAttemptRepository,
    rules::{
        self, AttemptKeys, Begun, Quota, Refusal, RowKey, Rules, Settlement, Tallies, Tally,
        written,
    },
};
use crate::{
    domain::iam::SigninLimits,
    shared::{Result, time::TimestampMicros},
};

#[derive(Default)]
pub(crate) struct MemoryAttempts(Mutex<HashMap<RowKey, Tally>>);

impl MemoryAttempts {
    fn tallies(&self, keys: &AttemptKeys) -> Tallies {
        let rows = self.0.lock().unwrap();
        let find = |key: Option<RowKey>| key.and_then(|key| rows.get(&key).copied());

        Tallies {
            account: find(Some(keys.account_row())),
            source: find(keys.source_row()),
            pair: find(keys.pair_row()),
        }
    }

    fn write(
        &self,
        keys: &AttemptKeys,
        before: &Tallies,
        after: &Tallies,
        rules: &Rules,
        now: TimestampMicros,
    ) {
        let mut rows = self.0.lock().unwrap();

        for row in written(keys, before, after, rules, now) {
            rows.insert(row.key, row.tally);
        }
    }
}

#[async_trait]
impl SigninAttemptRepository for MemoryAttempts {
    async fn refusal(
        &self,
        keys: &AttemptKeys,
        limits: &SigninLimits,
        now: TimestampMicros,
    ) -> Result<Option<Refusal>> {
        Ok(rules::refusal(
            &self.tallies(keys),
            &Rules::from(limits),
            now,
        ))
    }

    async fn begin(
        &self,
        keys: &AttemptKeys,
        limits: &SigninLimits,
        now: TimestampMicros,
    ) -> Result<Begun> {
        let rules = Rules::from(limits);
        let before = self.tallies(keys);
        let (after, begun) = rules::begin(before, keys.source.is_some(), &rules, now);

        self.write(keys, &before, &after, &rules, now);

        Ok(begun)
    }

    async fn settle(
        &self,
        keys: &AttemptKeys,
        limits: &SigninLimits,
        settlement: Settlement,
        now: TimestampMicros,
    ) -> Result<()> {
        let rules = Rules::from(limits);
        let before = self.tallies(keys);
        let after = rules::settled(before, keys.source.is_some(), settlement, &rules, now);

        self.write(keys, &before, &after, &rules, now);

        Ok(())
    }

    async fn forgive(&self, _: &AttemptKeys, _: TimestampMicros) -> Result<()> {
        unimplemented!("no test forgives through the in-memory attempts")
    }

    async fn take_quota(&self, _: &AttemptKeys, _: &Quota, _: TimestampMicros) -> Result<bool> {
        unimplemented!("no test takes a mail quota through the in-memory attempts")
    }
}
