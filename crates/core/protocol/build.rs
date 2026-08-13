// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Generates the workspace protocol bindings into Cargo's `OUT_DIR`.

use std::path::PathBuf;

const PROTO_FILES: &[&str] = &[
    "cluster/v1/cluster.proto",
    "intake/v1/intake.proto",
    "pprof/v1/profile.proto",
    "probe/v1/probe.proto",
    "query/v1/query.proto",
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../proto");
    let protos = PROTO_FILES
        .iter()
        .map(|path| proto_root.join(path))
        .collect::<Vec<_>>();
    for proto in &protos {
        println!("cargo:rerun-if-changed={}", proto.display());
    }

    tonic_prost_build::configure()
        .bytes(".")
        // Debian bullseye ships protoc 3.12, which requires this compatibility
        // flag for the proto3 optional fields used by probe.proto.
        .protoc_arg("--experimental_allow_proto3_optional")
        .compile_protos(&protos, std::slice::from_ref(&proto_root))?;
    Ok(())
}
