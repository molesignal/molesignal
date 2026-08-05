// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::{Value, json};

use crate::domain::dashboard::authoring::{
    AuthoringSize, GridDimensions, VisualizationCapability, VisualizationManifest,
};

#[derive(Default)]
pub(super) struct LayoutCursor {
    x: u32,
    y: u32,
    row_height: u32,
}

impl LayoutCursor {
    pub(super) fn place(&mut self, dimensions: GridDimensions, columns: u32) -> Value {
        if self.x > 0 && self.x.saturating_add(dimensions.w) > columns {
            self.y = self.y.saturating_add(self.row_height);
            self.x = 0;
            self.row_height = 0;
        }
        let position = json!({
            "x": self.x,
            "y": self.y,
            "w": dimensions.w,
            "h": dimensions.h
        });
        self.x = self.x.saturating_add(dimensions.w);
        self.row_height = self.row_height.max(dimensions.h);
        if self.x >= columns {
            self.y = self.y.saturating_add(self.row_height);
            self.x = 0;
            self.row_height = 0;
        }
        position
    }

    pub(super) fn bottom(&self) -> u32 {
        self.y.saturating_add(self.row_height)
    }
}

pub(super) fn panel_dimensions(
    size: Option<AuthoringSize>,
    capability: &VisualizationCapability,
    manifest: &VisualizationManifest,
) -> GridDimensions {
    let size = match size {
        Some(value) => value.as_str(),
        None => capability.default_size.as_str(),
    };
    dimensions_for(size, manifest)
}

pub(super) fn text_dimensions(
    size: Option<AuthoringSize>,
    manifest: &VisualizationManifest,
) -> GridDimensions {
    dimensions_for(size.map_or("full", AuthoringSize::as_str), manifest)
}

fn dimensions_for(size: &str, manifest: &VisualizationManifest) -> GridDimensions {
    let sizes = &manifest.grid.sizes;
    match size {
        "small" => sizes.small,
        "medium" => sizes.medium,
        "wide" => sizes.wide,
        _ => sizes.full,
    }
}
