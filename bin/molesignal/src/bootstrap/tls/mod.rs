// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! TLS server-side glue（change `domain-acme-tls`，仅  feature）。

pub mod sni_resolver;

pub use sni_resolver::SniCertResolver;

use crate::shared::{Error, Result};

/// Select the process-wide rustls provider before any TLS client or server is built.
///
/// The dependency graph enables both AWS-LC and ring, so rustls cannot infer a provider.
/// Installation is idempotent to support binaries and tests sharing the bootstrap path.
pub fn install_crypto_provider() -> Result<()> {
    use rustls::crypto::CryptoProvider;

    if CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    }
    if CryptoProvider::get_default().is_none() {
        return Err(Error::internal(
            "failed to install the process-wide rustls AWS-LC CryptoProvider",
        ));
    }
    Ok(())
}
