// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Parsed debug artifacts reused across every matching frame in one ingest request.

use super::frame::FramePlan;
use crate::{
    domain::rum::DebugArtifactKind,
    infra::rum::symbolication::{
        AndroidMapping, JavascriptSourceMap, NativeSymbolicator, OriginalFrame,
    },
    shared::Result,
};

pub(super) enum PreparedArtifact {
    Javascript(Box<JavascriptSourceMap>),
    AndroidMapping(AndroidMapping),
    Native(Box<NativeSymbolicator>),
}

impl PreparedArtifact {
    pub(super) fn parse(kind: DebugArtifactKind, bytes: &[u8]) -> Result<Self> {
        match kind {
            DebugArtifactKind::JavascriptSourcemap => JavascriptSourceMap::parse(bytes)
                .map(Box::new)
                .map(Self::Javascript),
            DebugArtifactKind::AndroidMapping => {
                AndroidMapping::parse(bytes).map(Self::AndroidMapping)
            }
            DebugArtifactKind::FlutterSymbols
            | DebugArtifactKind::AndroidNativeSymbols
            | DebugArtifactKind::AppleDsym => NativeSymbolicator::new(bytes)
                .map(Box::new)
                .map(Self::Native),
        }
    }

    pub(super) fn translate(
        &self,
        plan: &FramePlan,
    ) -> Result<Option<(OriginalFrame, Option<String>)>> {
        match self {
            Self::Javascript(source_map) => Ok(Some((
                source_map.translate(
                    plan.line.expect("JavaScript plan line"),
                    plan.column.expect("JavaScript plan column"),
                ),
                None,
            ))),
            Self::AndroidMapping(mapping) => Ok(mapping
                .translate(
                    plan.class_name.as_deref().expect("Android plan class"),
                    plan.function.as_deref().expect("Android plan function"),
                    plan.line,
                )
                .map(|restored| {
                    (
                        OriginalFrame {
                            file: Some(restored.class_name.clone()),
                            function: Some(restored.function),
                            line: restored.line,
                            column: None,
                        },
                        Some(restored.class_name),
                    )
                })),
            Self::Native(symbolicator) => symbolicator
                .translate(plan.address.expect("native plan address"))
                .map(|frame| Some((frame, None))),
        }
    }
}
