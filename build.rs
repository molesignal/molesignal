// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 单-crate 合并后的构建脚本。
//!
//! 捕获 git commit / branch、CI build ID 与构建时间戳，注入为 rustc-env，供
//! `/version` 接口回显（前端「关于」弹窗 + 状态栏）。非 git 环境（如发布 tarball）回退到
//! "unknown"，不影响编译。
//!
//! protobuf 代码生成现改为手动执行：`cd proto && buf generate`（输出到
//! `src/protocol/`，见
//! `proto/buf.gen.yaml`）。生成产物随源码提交，普通构建无需 Buf CLI/网络。

use std::{env, process::Command};

const INTERNAL_PRODUCT_VERSION: &str = "26.0.0.0";

fn main() {
    let git = |args: &[&str]| -> String {
        Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "unknown".to_string())
    };
    let commit = env::var("GIT_SHA")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| git(&["rev-parse", "HEAD"]));
    let branch = git(&["rev-parse", "--abbrev-ref", "HEAD"]);
    let build_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let build_id = env::var("BUILD_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=MOLESIGNAL_GIT_COMMIT={commit}");
    println!("cargo:rustc-env=MOLESIGNAL_GIT_BRANCH={branch}");
    println!("cargo:rustc-env=MOLESIGNAL_BUILD_EPOCH={build_epoch}");
    println!("cargo:rustc-env=MOLESIGNAL_BUILD_ID={build_id}");
    println!("cargo:rustc-env=MOLESIGNAL_PRODUCT_VERSION={INTERNAL_PRODUCT_VERSION}");
    // Best-effort: refresh git info when HEAD moves (ignored if path absent).
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-env-changed=BUILD_ID");
    println!("cargo:rerun-if-env-changed=GIT_SHA");
}
