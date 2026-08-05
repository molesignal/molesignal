// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! ReportRenderer 端口（spec scheduled-reports / change `scheduled-reports-headless`）。
//!
//! 在 `shared` 而非 `app`/`infra`，因为 API 即时导出与
//! `ScheduledReportRunner` 定时投递都需要调用同一渲染端口。

use async_trait::async_trait;
use bytes::Bytes;

use crate::shared::Result;

/// 输出格式；只接 image / PDF（其它格式不走 renderer，由 runner 自己产）。
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
    type Err = crate::shared::Error;

    /// `"png"` / `"pdf"` 字面量解析；其它输入 → error。
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "png" => Ok(Self::Png),
            "pdf" => Ok(Self::Pdf),
            _ => Err(crate::shared::Error::invalid(format!(
                "unknown report render format: {s}"
            ))),
        }
    }
}

/// Render viewport 像素尺寸（headless Chrome `setDeviceMetricsOverride`）。
#[derive(Debug, Clone, Copy)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 800,
        }
    }
}

/// 渲染失败原因；caller 用来填 `report_deliveries.error`。
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

impl From<RenderError> for crate::shared::Error {
    fn from(e: RenderError) -> Self {
        match e {
            RenderError::Timeout(_) | RenderError::PageError(_) => {
                crate::shared::Error::invalid(e.to_string())
            }
            RenderError::ChromeUnavailable(_) | RenderError::Internal(_) => {
                crate::shared::Error::internal(e.to_string())
            }
        }
    }
}

/// 给 ScheduledReportRunner 用的渲染入口。
///
/// 实装挂在  crate；OSS build 永远 `None`。
#[async_trait]
pub trait ReportRenderer: Send + Sync {
    /// 把 `url` 加载到 headless Chrome 中，按 `format` 截图或打印为 PDF。
    ///
    /// `browser_auth_storage` 是临时写入浏览器 `molesignal-auth` 同源存储的
    /// Zustand persist JSON；仅交给 Chrome 进程，不拼入 URL，打印后立即清除。
    ///
    /// 返 raw bytes；caller 决定怎么 deliver（webhook POST / s3 PUT / email attach）。
    async fn render(
        &self,
        url: &str,
        format: ReportFormat,
        viewport: Viewport,
        browser_auth_storage: Option<&str>,
    ) -> Result<Bytes>;
}

/// 在声明 PDF/PNG MIME 前校验 renderer 的 magic bytes，避免把错误页、JSON 或 SVG
/// 伪装成对应文件下载。
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
        Err(crate::shared::Error::internal(format!(
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
