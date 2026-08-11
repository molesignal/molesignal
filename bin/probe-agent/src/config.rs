// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "probe-agent", version, about)]
pub struct Cli {
    #[arg(long, global = true, default_value = "/var/lib/molesignal-probe")]
    pub state_dir: PathBuf,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Register this Agent once and persist its private mTLS identity locally.
    Register {
        #[arg(long)]
        endpoint: String,
        #[arg(long)]
        token: String,
        #[arg(long)]
        ca_certificate_base64: String,
        #[arg(long)]
        hostname: Option<String>,
    },
    /// Connect to MoleSignal and execute Probe tasks until stopped.
    Run {
        #[arg(long, default_value_t = 4)]
        max_concurrent: u32,
        #[arg(long, default_value_t = 1)]
        max_browser_concurrent: u32,
    },
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;

    use super::{Cli, Command};

    #[test]
    fn parses_register_subcommand() {
        let cli = Cli::try_parse_from([
            "probe-agent",
            "register",
            "--endpoint",
            "https://molesignal.example.com:5084",
            "--token",
            "token",
            "--ca-certificate-base64",
            "certificate",
        ])
        .expect("register command should parse");

        assert!(matches!(cli.command, Command::Register { .. }));
    }
}
