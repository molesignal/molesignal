// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! ReportRenderer port (spec scheduled-reports / change `scheduled-reports-headless`).
//!
//! It lives in `shared` rather than `app`/`infra`, because the API instant export and the
//! `ScheduledReportRunner` scheduled delivery both need to call the same render port.

use async_trait::async_trait;
use bytes::Bytes;
use kernel::Result;

mod chrome;

pub use chrome::{HeadlessChromeRenderer, MAX_POOL_SIZE, MAX_TIMEOUT_SECS, RendererConfig};

/// Output format; only image / PDF are accepted (other formats skip the renderer and are
/// produced by the runner itself).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFormat {
    Png,
    Pdf,
}

impl ReportFormat {
    pub fn content_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Pdf => "application/pdf",
        }
    }
}

impl std::str::FromStr for ReportFormat {
    type Err = kernel::Error;

    /// Parses the `"png"` / `"pdf"` literals; any other input returns an error.
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "png" => Ok(Self::Png),
            "pdf" => Ok(Self::Pdf),
            _ => Err(kernel::Error::invalid(format!(
                "unknown report render format: {s}"
            ))),
        }
    }
}

/// Render viewport size in pixels (headless Chrome `setDeviceMetricsOverride`).
#[derive(Debug, Clone, Copy)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
}

impl Viewport {
    /// Use the renderer's own configured viewport (`[scheduled_reports.renderer].viewport_*`).
    /// Pass it whenever the caller has no reason to deviate from the deployment config;
    /// passing a specific size overrides the configuration.
    pub const CONFIGURED: Self = Self {
        width: 0,
        height: 0,
    };
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 800,
        }
    }
}

/// Reason a render failed; the caller uses it to fill `report_deliveries.error`.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("render timeout after {0}s")]
    Timeout(u64),
    #[error("chrome unavailable: {0}")]
    ChromeUnavailable(String),
    #[error("page error: {0}")]
    PageError(String),
    #[error("renderer internal error: {0}")]
    Internal(String),
}

impl From<RenderError> for kernel::Error {
    fn from(e: RenderError) -> Self {
        match e {
            RenderError::Timeout(_) | RenderError::PageError(_) => {
                kernel::Error::invalid(e.to_string())
            }
            RenderError::ChromeUnavailable(_) | RenderError::Internal(_) => {
                kernel::Error::internal(e.to_string())
            }
        }
    }
}

/// Render entry point for ScheduledReportRunner.
///
/// The concrete implementation is injected by the composition root.
#[async_trait]
pub trait ReportRenderer: Send + Sync {
    /// Loads `url` in headless Chrome and, per `format`, takes a screenshot or prints to PDF.
    ///
    /// `viewport` is the viewport used when the page loads; [`Viewport::CONFIGURED`] means the
    /// size the renderer is configured with.
    /// The output width equals the viewport width, and the height follows the page content.
    ///
    /// `session` is the sign-in cookie for this render: it is written only into the Chrome
    /// instance borrowed for this render, never put in the URL, and cleared right after printing.
    ///
    /// Returns the raw bytes; the caller decides how to deliver them
    /// (webhook POST / s3 PUT / email attach).
    async fn render(
        &self,
        url: &str,
        format: ReportFormat,
        viewport: Viewport,
        session: Option<&SessionCookie>,
    ) -> Result<Bytes>;
}

/// The sign-in cookie the browser carries when rendering a report page.
#[derive(Debug, Clone)]
pub struct SessionCookie {
    pub name: String,
    pub value: String,
    /// The cookie's `Path`.
    pub path: String,
}

/// Validates the renderer's magic bytes before declaring a PDF/PNG MIME type, so that an error
/// page, JSON, or SVG cannot be passed off as the corresponding file download.
pub fn validate_report_bytes(format: ReportFormat, bytes: &[u8]) -> Result<()> {
    let valid = match format {
        ReportFormat::Pdf => bytes.starts_with(b"%PDF-"),
        ReportFormat::Png => {
            bytes.starts_with(&[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'])
        }
    };

    if valid {
        Ok(())
    } else {
        Err(kernel::Error::internal(format!(
            "report renderer returned invalid {} payload",
            format.content_type()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_pdf_and_png_magic_bytes() {
        assert!(validate_report_bytes(ReportFormat::Pdf, b"%PDF-1.7\n").is_ok());
        assert!(
            validate_report_bytes(
                ReportFormat::Png,
                &[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n']
            )
            .is_ok()
        );
        assert!(validate_report_bytes(ReportFormat::Pdf, br#"{"error":"nope"}"#).is_err());
        assert!(validate_report_bytes(ReportFormat::Png, b"<svg/>").is_err());
    }
}
