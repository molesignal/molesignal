// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use crate::config::Settings;

pub(super) fn artifact_base_url(settings: &Settings) -> String {
    let external = settings.http.external_url.trim();
    if !external.is_empty() {
        return external.trim_end_matches('/').to_string();
    }
    if settings.http.tls.enabled {
        format!("https://localhost:{}", settings.http.tls.port)
    } else {
        format!("http://localhost:{}", settings.http.port)
    }
}
