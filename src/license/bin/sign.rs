// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Offline issuer CLI for  license files.
//!
//! Build with the `sign` feature; the binary is excluded from default builds so
//! the runtime crate stays minimal:
//!
//! ```text
//! cargo run -p molesignal-license --features sign --bin msi-license -- <subcommand>
//! ```
//!
//! Subcommands:
//! - `keygen`   — generate a fresh Ed25519 keypair (hex-encoded on stdout).
//! - `sign`     — sign a `LicensePayload` JSON file → `license.json`.
//! - `verify`   — round-trip a license file through `SignedLicense::verify`.
//!
//! Signing key sources (checked in order): `--key-hex`, `--key-file`,
//! `MS_LICENSE_SIGNING_KEY_HEX` env. Public key sources for `verify`:
//! `--pubkey-hex`, `--pubkey-file`, `MS_LICENSE_ROOT_PUBKEY_HEX` env.

use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Context, Result, anyhow, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use clap::{Parser, Subcommand};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use crate::license::{LicenseFile, LicensePayload, SignedLicense};
// rand 0.10 no longer re-exports RngCore at the crate root; the fill_bytes
// method is on the renamed Rng trait.
use rand::Rng;

const SIGNING_KEY_ENV: &str = "MS_LICENSE_SIGNING_KEY_HEX";
const ROOT_PUBKEY_ENV: &str = "MS_LICENSE_ROOT_PUBKEY_HEX";

#[derive(Parser, Debug)]
#[command(
    name = "msi-license",
    about = "Molesignal  license issuer tool",
    version
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Generate a fresh Ed25519 keypair. Writes `private_hex\npublic_hex\n` to stdout.
    Keygen {
        /// Optional file to write the 32-byte hex private key to (mode 0600 recommended).
        #[arg(long)]
        out_private: Option<PathBuf>,
        /// Optional file to write the 32-byte hex public key to.
        #[arg(long)]
        out_public: Option<PathBuf>,
    },
    /// Sign a payload JSON file and emit a license JSON.
    Sign {
        /// Path to a JSON file shaped like `LicensePayload`.
        #[arg(long)]
        payload: PathBuf,
        /// 32-byte signing key as hex (avoid in shell history — prefer env or --key-file).
        #[arg(long, conflicts_with_all = ["key_file"])]
        key_hex: Option<String>,
        /// Path to a file containing the 32-byte hex signing key (single line).
        #[arg(long)]
        key_file: Option<PathBuf>,
        /// Where to write the signed `license.json`. Defaults to stdout.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Verify a signed license file with a known root public key.
    Verify {
        /// Path to a signed `license.json`.
        #[arg(long)]
        license: PathBuf,
        /// 32-byte verifying key as hex.
        #[arg(long, conflicts_with_all = ["pubkey_file"])]
        pubkey_hex: Option<String>,
        /// Path to a file containing the 32-byte hex verifying key (single line).
        #[arg(long)]
        pubkey_file: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Keygen {
            out_private,
            out_public,
        } => cmd_keygen(out_private, out_public),
        Cmd::Sign {
            payload,
            key_hex,
            key_file,
            out,
        } => cmd_sign(payload, key_hex, key_file, out),
        Cmd::Verify {
            license,
            pubkey_hex,
            pubkey_file,
        } => cmd_verify(license, pubkey_hex, pubkey_file),
    }
}

fn cmd_keygen(out_private: Option<PathBuf>, out_public: Option<PathBuf>) -> Result<()> {
    let mut seed = [0u8; 32];
    rand::rng().fill_bytes(&mut seed);
    let sk = SigningKey::from_bytes(&seed);
    let vk = sk.verifying_key();
    let sk_hex = hex::encode(sk.to_bytes());
    let vk_hex = hex::encode(vk.to_bytes());

    if let Some(p) = &out_private {
        fs::write(p, format!("{sk_hex}\n")).with_context(|| format!("write {}", p.display()))?;
    }
    if let Some(p) = &out_public {
        fs::write(p, format!("{vk_hex}\n")).with_context(|| format!("write {}", p.display()))?;
    }

    let mut stdout = io::stdout().lock();
    writeln!(stdout, "private_hex={sk_hex}")?;
    writeln!(stdout, "public_hex={vk_hex}")?;
    writeln!(
        stdout,
        "\n# Treat private_hex as a secret. Embed public_hex into the binary's root pubkey."
    )?;
    Ok(())
}

fn cmd_sign(
    payload_path: PathBuf,
    key_hex: Option<String>,
    key_file: Option<PathBuf>,
    out: Option<PathBuf>,
) -> Result<()> {
    let raw = fs::read_to_string(&payload_path)
        .with_context(|| format!("read payload {}", payload_path.display()))?;
    let payload: LicensePayload = serde_json::from_str(&raw).context("parse payload JSON")?;
    if payload.issued_to.trim().is_empty() {
        bail!("payload.issued_to must be non-empty");
    }

    let sk_bytes = load_32_byte_hex(key_hex, key_file.as_deref(), SIGNING_KEY_ENV, "signing key")?;
    let sk = SigningKey::from_bytes(&sk_bytes);

    let payload_bytes = serde_json::to_vec(&payload).context("re-serialize payload")?;
    let payload_b64 = B64.encode(&payload_bytes);
    let signature = sk.sign(payload_b64.as_bytes());
    let signature_b64 = B64.encode(signature.to_bytes());

    let file = LicenseFile {
        payload_b64,
        signature_b64,
    };

    // Round-trip through the runtime verifier so we never ship a license that
    // would fail at load time.
    let pubkey = sk.verifying_key().to_bytes();
    SignedLicense::verify(&file, &pubkey).context("self-verify produced license")?;

    let serialized = serde_json::to_vec_pretty(&file)?;
    match out {
        Some(p) => {
            fs::write(&p, &serialized).with_context(|| format!("write {}", p.display()))?;
            eprintln!("wrote {}", p.display());
        }
        None => io::stdout().lock().write_all(&serialized)?,
    }
    Ok(())
}

fn cmd_verify(
    license_path: PathBuf,
    pubkey_hex: Option<String>,
    pubkey_file: Option<PathBuf>,
) -> Result<()> {
    let pk_bytes = load_32_byte_hex(
        pubkey_hex,
        pubkey_file.as_deref(),
        ROOT_PUBKEY_ENV,
        "root public key",
    )?;
    // Ensure the bytes form a valid Ed25519 point before we hand them off.
    VerifyingKey::from_bytes(&pk_bytes).context("root public key not a valid Ed25519 point")?;

    let raw = fs::read_to_string(&license_path)
        .with_context(|| format!("read license {}", license_path.display()))?;
    let file: LicenseFile = serde_json::from_str(&raw).context("parse license JSON")?;
    let lic = SignedLicense::verify(&file, &pk_bytes).context("signature verification failed")?;

    let payload = lic.payload();
    println!("ok");
    println!("issued_to               = {}", payload.issued_to);
    println!("expires_at_micros       = {}", payload.expires_at_micros);
    println!(
        "max_ingest_bytes_per_day = {}",
        payload.max_ingest_bytes_per_day
    );
    println!("max_users               = {}", payload.max_users);
    println!("features                = {:?}", payload.features);
    Ok(())
}

fn load_32_byte_hex(
    arg_hex: Option<String>,
    file: Option<&std::path::Path>,
    env_var: &str,
    label: &str,
) -> Result<[u8; 32]> {
    let hex_str = if let Some(s) = arg_hex {
        s
    } else if let Some(p) = file {
        fs::read_to_string(p).with_context(|| format!("read {} from {}", label, p.display()))?
    } else {
        std::env::var(env_var).map_err(|_| {
            anyhow!(
                "no {label} supplied (pass --{kind}-hex, --{kind}-file, or set {env_var})",
                kind = if label.contains("public") {
                    "pubkey"
                } else {
                    "key"
                }
            )
        })?
    };
    let trimmed = hex_str.trim();
    let bytes = hex::decode(trimmed).with_context(|| format!("{label} is not valid hex"))?;
    if bytes.len() != 32 {
        bail!("{label} must decode to 32 bytes, got {}", bytes.len());
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}
