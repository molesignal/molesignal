// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::shared::{Result, ids::Id, time::TimestampMicros};

/// Build artifact used to restore minified, obfuscated, or native RUM stacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DebugArtifactKind {
    JavascriptSourcemap,
    FlutterSymbols,
    AndroidMapping,
    AndroidNativeSymbols,
    AppleDsym,
}

impl DebugArtifactKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::JavascriptSourcemap => "javascript_sourcemap",
            Self::FlutterSymbols => "flutter_symbols",
            Self::AndroidMapping => "android_mapping",
            Self::AndroidNativeSymbols => "android_native_symbols",
            Self::AppleDsym => "apple_dsym",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "javascript_sourcemap" => Some(Self::JavascriptSourcemap),
            "flutter_symbols" => Some(Self::FlutterSymbols),
            "android_mapping" => Some(Self::AndroidMapping),
            "android_native_symbols" => Some(Self::AndroidNativeSymbols),
            "apple_dsym" => Some(Self::AppleDsym),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugArtifactMeta {
    pub id: Id,
    pub org_id: Id,
    pub application_id: String,
    pub service: String,
    pub release: String,
    pub kind: DebugArtifactKind,
    pub platform: String,
    pub architecture: String,
    pub debug_id: String,
    pub filename: String,
    pub object_key: String,
    pub size_bytes: u64,
    pub checksum_sha256: String,
    pub uploaded_at: TimestampMicros,
}

#[derive(Debug, Clone)]
pub struct DebugArtifactUpsert {
    pub artifact: DebugArtifactMeta,
    pub replaced_object_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DebugArtifactLookup<'a> {
    pub application_id: &'a str,
    pub service: &'a str,
    pub release: &'a str,
    pub kind: DebugArtifactKind,
    pub platform: Option<&'a str>,
    pub architecture: Option<&'a str>,
    pub debug_id: Option<&'a str>,
    pub filename: Option<&'a str>,
}

#[async_trait]
pub trait DebugArtifactRepository: Send + Sync {
    async fn create(&self, artifact: DebugArtifactMeta) -> Result<DebugArtifactUpsert>;

    async fn list(
        &self,
        org_id: &Id,
        application_id: Option<&str>,
        service: Option<&str>,
        kind: Option<DebugArtifactKind>,
        platform: Option<&str>,
    ) -> Result<Vec<DebugArtifactMeta>>;

    async fn find_best(
        &self,
        org_id: &Id,
        lookup: &DebugArtifactLookup<'_>,
    ) -> Result<Option<DebugArtifactMeta>>;

    async fn delete(&self, org_id: &Id, id: &Id) -> Result<Option<DebugArtifactMeta>>;
}
