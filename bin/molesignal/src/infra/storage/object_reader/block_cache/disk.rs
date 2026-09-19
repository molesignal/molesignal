// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{ffi::CString, os::unix::ffi::OsStrExt, path::Path};

pub(super) fn available_bytes(path: &Path) -> Option<u64> {
    let path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `path` is a valid, NUL-terminated filesystem path and `stats` points to writable
    // storage for one `statvfs` value. The value is read only when libc reports success.
    let result = unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) };
    (result == 0).then(|| {
        // SAFETY: a successful `statvfs` call initialized the output value.
        let stats = unsafe { stats.assume_init() };
        let available_bytes = u128::from(stats.f_bavail).saturating_mul(u128::from(stats.f_frsize));
        u64::try_from(available_bytes).unwrap_or(u64::MAX)
    })
}
