// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Generates the Probe Agent protocol bindings into Cargo's `OUT_DIR`.

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../proto");
    let proto = proto_root.join("probe/v1/probe.proto");
    println!("cargo:rerun-if-changed={}", proto.display());

    tonic_prost_build::configure()
        .bytes(".")
        .compile_protos(&[proto], std::slice::from_ref(&proto_root))?;
    Ok(())
}
