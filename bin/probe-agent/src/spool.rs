// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{fs, path::PathBuf};

use anyhow::{Context as _, Result};
use prost::Message as _;
use sha2::{Digest as _, Sha256};

use crate::protocol::v1::ProbeResult;

#[derive(Clone)]
pub struct ResultSpool {
    directory: PathBuf,
}

pub struct SpoolEntry {
    pub result: ProbeResult,
}

impl ResultSpool {
    pub fn open(state_dir: PathBuf) -> Result<Self> {
        let directory = state_dir.join("results");
        fs::create_dir_all(&directory).context("create Probe result spool")?;
        restrict_directory(&directory)?;
        Ok(Self { directory })
    }

    pub fn store(&self, result: &ProbeResult) -> Result<()> {
        let path = self.path_for(&result.task_id, result.result_sequence);
        let temporary = path.with_extension("tmp");
        fs::write(&temporary, result.encode_to_vec()).context("write Probe result spool")?;
        restrict_file(&temporary)?;
        fs::rename(temporary, path).context("commit Probe result spool")
    }

    pub fn pending(&self) -> Result<Vec<SpoolEntry>> {
        let mut paths = fs::read_dir(&self.directory)
            .context("read Probe result spool")?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().is_some_and(|extension| extension == "pb"))
            .collect::<Vec<_>>();
        paths.sort();
        paths
            .into_iter()
            .map(|path| {
                let bytes = fs::read(&path)
                    .with_context(|| format!("read spooled result {}", path.display()))?;
                let result = ProbeResult::decode(bytes.as_slice())
                    .with_context(|| format!("decode spooled result {}", path.display()))?;
                Ok(SpoolEntry { result })
            })
            .collect()
    }

    pub fn acknowledge(&self, task_id: &str, result_sequence: u64) -> Result<()> {
        let path = self.path_for(task_id, result_sequence);
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).context("remove acknowledged Probe result"),
        }
    }

    pub fn size_bytes(&self) -> u64 {
        fs::read_dir(&self.directory)
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| entry.metadata().ok())
            .map(|metadata| metadata.len())
            .sum()
    }

    fn path_for(&self, task_id: &str, result_sequence: u64) -> PathBuf {
        let task_hash = hex::encode(Sha256::digest(task_id.as_bytes()));
        self.directory
            .join(format!("{result_sequence:020}-{task_hash}.pb"))
    }
}

#[cfg(unix)]
fn restrict_directory(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).context("chmod Probe spool")
}

#[cfg(not(unix))]
fn restrict_directory(_path: &std::path::Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn restrict_file(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).context("chmod Probe result")
}

#[cfg(not(unix))]
fn restrict_file(_path: &std::path::Path) -> Result<()> {
    Ok(())
}
