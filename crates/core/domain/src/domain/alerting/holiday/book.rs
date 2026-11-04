// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 进程里的节假日缓存：每个节点启动时读一遍全部日历，之后定时整份换掉；本节点改了某份日历时
//! 立即换掉那一份。时间段判断某一天时按日历 id 查这里。
//!
//! 整份是写时复制的：读的人拿到一个 `Arc`，换的时候不挡住正在判断的人。

use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, PoisonError, RwLock},
};

use chrono::NaiveDate;

use super::{DayKind, HolidayCalendar};
use crate::shared::ids::Id;

#[derive(Debug, Clone)]
struct Entry {
    org_id: Id,
    days: HashMap<NaiveDate, DayKind>,
}

type Book = HashMap<Id, Arc<Entry>>;

static BOOK: LazyLock<RwLock<Arc<Book>>> = LazyLock::new(|| RwLock::new(Arc::new(Book::new())));

fn entry_of(calendar: &HolidayCalendar) -> Arc<Entry> {
    Arc::new(Entry {
        org_id: calendar.org_id.clone(),
        days: calendar
            .days
            .iter()
            .map(|day| (day.date, day.kind))
            .collect(),
    })
}

fn current() -> Arc<Book> {
    BOOK.read().unwrap_or_else(PoisonError::into_inner).clone()
}

fn change(apply: impl FnOnce(&mut Book)) {
    let mut guard = BOOK.write().unwrap_or_else(PoisonError::into_inner);
    let mut next = (**guard).clone();

    apply(&mut next);
    *guard = Arc::new(next);
}

/// 换成这些日历：定时刷新时用。
pub fn replace_all(calendars: &[HolidayCalendar]) {
    let next: Book = calendars
        .iter()
        .map(|calendar| (calendar.id.clone(), entry_of(calendar)))
        .collect();

    *BOOK.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(next);
}

/// 记下（或换掉）一份日历。
pub fn remember(calendar: &HolidayCalendar) {
    let entry = entry_of(calendar);

    change(|book| {
        book.insert(calendar.id.clone(), entry);
    });
}

/// 忘掉一份删掉的日历。
pub fn forget(id: &Id) {
    change(|book| {
        book.remove(id);
    });
}

/// 这份日历里这一天是放假还是补班；不是节假日、或没有这份日历时为 `None`。
pub fn day_kind(calendar_id: &Id, date: NaiveDate) -> Option<DayKind> {
    current()
        .get(calendar_id)
        .and_then(|entry| entry.days.get(&date).copied())
}

/// 有这份日历，并且是这个组织的。
pub fn belongs_to(calendar_id: &Id, org_id: &Id) -> bool {
    current()
        .get(calendar_id)
        .is_some_and(|entry| entry.org_id == *org_id)
}
