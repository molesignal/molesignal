// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use anyhow::{Result, ensure};
use clap::Parser as _;
use probe_agent::{
    config::{Cli, Command},
    runtime::{RegisterOptions, RunOptions, register, run},
};

#[tokio::main]
async fn main() -> Result<()> {
    install_crypto_provider()?;
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "probe_agent=info".into()),
        )
        .init();
    let cli = Cli::parse();
    match cli.command {
        Command::Register {
            endpoint,
            token,
            ca_certificate_base64,
            hostname,
        } => {
            register(RegisterOptions {
                state_dir: cli.state_dir,
                endpoint,
                token,
                ca_certificate_base64,
                hostname,
            })
            .await
        }
        Command::Run {
            max_concurrent,
            max_browser_concurrent,
        } => {
            run(RunOptions {
                state_dir: cli.state_dir,
                max_concurrent,
                max_browser_concurrent,
            })
            .await
        }
    }
}

fn install_crypto_provider() -> Result<()> {
    use rustls::crypto::CryptoProvider;

    if CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    }
    ensure!(
        CryptoProvider::get_default().is_some(),
        "failed to install the process-wide rustls AWS-LC CryptoProvider"
    );
    Ok(())
}
