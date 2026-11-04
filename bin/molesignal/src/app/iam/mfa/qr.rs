// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 设置身份验证器时扫的二维码，画成 SVG：页面用 `<img>` 显示，不需要前端的二维码库。

use qrcode::{EcLevel, QrCode, render::svg};

use crate::shared::{Error, Result};

/// `text` 的二维码。黑码白底，留出静区，深色界面里也能扫。
pub fn svg(text: &str) -> Result<String> {
    let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M)
        .map_err(|error| Error::internal(format!("qr code: {error}")))?;

    Ok(code
        .render::<svg::Color<'_>>()
        .min_dimensions(200, 200)
        .quiet_zone(true)
        .dark_color(svg::Color("#000000"))
        .light_color(svg::Color("#ffffff"))
        .build())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_authenticator_address_becomes_an_svg_image() {
        let image = svg("otpauth://totp/MoleSignal:alice%40example.com?secret=MZXW6YTBOI").unwrap();

        assert!(image.contains("<svg"));
        assert!(image.contains("#000000"));
    }
}
