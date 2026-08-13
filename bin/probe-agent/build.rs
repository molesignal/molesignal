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
        // Debian bullseye ships protoc 3.12, which requires this compatibility
        // flag for the proto3 optional fields used by probe.proto.
        .protoc_arg("--experimental_allow_proto3_optional")
        .compile_protos(&[proto], std::slice::from_ref(&proto_root))?;
    Ok(())
}
