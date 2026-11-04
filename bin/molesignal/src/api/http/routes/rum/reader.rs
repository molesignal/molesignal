// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Running the statements of the RUM pages. The reader is the app layer's, which the tools that
//! read what the pages read use too; here it is made from the state of the server.

use std::ops::Deref;

use crate::{
    api::AppState,
    app::{iam::IamContext, rum::reader},
};
pub(super) use crate::{
    app::rum::reader::{Counted, Span, within_budget},
    infra::rum::query::{Row, rows},
};

/// Runs statements for one reader. The query service masks what the reader may not see.
pub(super) struct Reader<'a>(reader::Reader<'a>);

impl<'a> Reader<'a> {
    pub(super) fn new(state: &'a AppState, iam: &'a IamContext) -> Self {
        Self(reader::Reader::new(
            &state.query,
            state.telemetry.streams.as_ref(),
            iam,
        ))
    }
}

impl<'a> Deref for Reader<'a> {
    type Target = reader::Reader<'a>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
