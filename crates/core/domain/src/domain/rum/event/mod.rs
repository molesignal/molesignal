// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The RUM event model: the kinds of event an SDK sends, the stream each is kept in, and the fixed
//! columns of those streams. The wire format is the JSON Schema in `contracts/rum/events/`; the
//! tables here say how an event is flattened into columns.

mod action;
mod common;
mod error;
mod field;
mod resource;
mod schema;
mod session;
mod stream;
mod view;

pub use field::{DEFAULT_TEXT_LEN, RumField, RumUnit};
pub use schema::{field, fields, stream_schema};
pub use stream::{RumEventType, RumStream, Versioned};
