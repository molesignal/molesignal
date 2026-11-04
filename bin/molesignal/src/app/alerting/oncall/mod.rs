// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 值班的人自己用的：订阅值班日历，请别人替自己值一段班。

pub mod calendar;
pub mod swaps;

pub use calendar::{CalendarEvent, CalendarFeeds, CalendarTexts, render_ics};
pub use swaps::{NewSwap, ShiftSwaps, SwapOutcome};
