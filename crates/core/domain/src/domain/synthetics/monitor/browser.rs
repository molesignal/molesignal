// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use super::{MonitorAssertion, ValueSource};
use crate::shared::ids::Id;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum BrowserAction {
    Navigate {
        url: ValueSource,
        wait_until: String,
    },
    Click {
        selector: String,
    },
    Fill {
        selector: String,
        value: ValueSource,
    },
    Select {
        selector: String,
        value: ValueSource,
    },
    WaitDuration {
        duration_millis: u32,
    },
    WaitSelector {
        selector: String,
    },
    WaitExpression {
        expression: String,
    },
    Extract {
        variable: String,
        selector: String,
        source: String,
    },
    Assert {
        assertion: MonitorAssertion,
        selector: Option<String>,
        page_expression: Option<String>,
    },
    Screenshot {
        name: String,
        full_page: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrowserStep {
    pub id: Id,
    pub name: String,
    pub action: BrowserAction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrowserJourneySpec {
    pub steps: Vec<BrowserStep>,
    pub viewport: Viewport,
    pub user_agent: Option<String>,
    pub capture_screenshot_on_failure: bool,
    pub capture_har_on_failure: bool,
    #[serde(default)]
    pub capture_trace_on_failure: bool,
}
