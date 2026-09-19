// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use super::{Extraction, MonitorAssertion, ValueSource};
use crate::shared::ids::Id;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeaderValue {
    pub name: String,
    pub value: ValueSource,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HttpStep {
    pub id: Id,
    pub name: String,
    pub method: String,
    pub url: ValueSource,
    #[serde(default)]
    pub headers: Vec<HeaderValue>,
    #[serde(default)]
    pub query: Vec<HeaderValue>,
    pub body: Option<ValueSource>,
    #[serde(default)]
    pub extractions: Vec<Extraction>,
    #[serde(default)]
    pub assertions: Vec<MonitorAssertion>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HttpJourneySpec {
    pub steps: Vec<HttpStep>,
    pub follow_redirects: bool,
    pub max_redirects: u8,
    pub verify_tls: bool,
}
