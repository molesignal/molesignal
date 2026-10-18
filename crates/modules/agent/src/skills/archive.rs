// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 解开 Skill 包：`.zip` / `.skill`（zip）与 `.tar.gz` / `.tgz`。
//!
//! 只接受普通文件与目录：绝对路径、`..` 一律拒绝；上传的包里有符号链接、硬链接或设备文件时
//! 整包拒绝，仓库压缩包里的这些条目略过不跟。文件数、单个文件与解开后的总大小都有上限，防
//! 压缩炸弹。macOS 打包时带的 `__MACOSX/` 与 `.DS_Store` 略过。仓库压缩包可以很大，先用
//! [`list_paths`] 看清有哪些文件，再用 `keep` 只留需要的那部分。

use std::io::{Cursor, Read};

use flate2::read::GzDecoder;

use super::{MAX_FILE_BYTES, MAX_FILES, MAX_UNPACKED_BYTES, SkillFile};

/// 一次解包留下多少文件、多少字节，遇到链接怎么办。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnpackLimits {
    pub max_files: usize,
    pub max_bytes: u64,
    pub skip_links: bool,
}

/// 上传的一个 Skill 包。
pub const PACKAGE_LIMITS: UnpackLimits = UnpackLimits {
    max_files: MAX_FILES,
    max_bytes: MAX_UNPACKED_BYTES,
    skip_links: false,
};

/// 一个仓库里的全部 Skill。
pub const REPOSITORY_LIMITS: UnpackLimits = UnpackLimits {
    max_files: 2_000,
    max_bytes: 128 * 1024 * 1024,
    skip_links: true,
};
use crate::shared::{Error, Result};

/// 读仓库压缩包时最多看多少个条目，留下的文件另有上限。
const MAX_ENTRIES: usize = 50_000;

/// 读仓库压缩包时最多解出多少字节（包括略过的文件）。
const MAX_SCANNED_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
    TarGz,
}

/// 由文件名判断格式。
pub fn archive_kind(file_name: &str) -> Option<ArchiveKind> {
    let name = file_name.trim().to_ascii_lowercase();

    if name.ends_with(".zip") || name.ends_with(".skill") {
        Some(ArchiveKind::Zip)
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        Some(ArchiveKind::TarGz)
    } else {
        None
    }
}

/// 解开后的文件，与仓库压缩包里记下的提交（tar 的 pax 全局头 `comment=`）。
#[derive(Debug, Default)]
pub struct Unpacked {
    pub files: Vec<SkillFile>,
    pub commit: Option<String>,
}

/// 包里的路径改成安全的相对路径；不安全或要略过时为空。
pub fn safe_path(raw: &str) -> Result<Option<String>> {
    let normalized = raw.replace('\\', "/");

    if normalized.starts_with('/') || normalized.as_bytes().get(1) == Some(&b':') {
        return Err(Error::invalid(format!(
            "the package holds an absolute path `{raw}`"
        )));
    }

    let mut parts = Vec::new();

    for part in normalized.split('/') {
        match part {
            "" | "." => continue,
            ".." => {
                return Err(Error::invalid(format!(
                    "the package holds a path that leaves it: `{raw}`"
                )));
            }
            part => parts.push(part),
        }
    }

    if parts.is_empty() || parts.first() == Some(&"__MACOSX") || parts.last() == Some(&".DS_Store")
    {
        return Ok(None);
    }

    Ok(Some(parts.join("/")))
}

struct Budget {
    limits: UnpackLimits,
    files: usize,
    kept_bytes: u64,
    scanned_bytes: u64,
    entries: usize,
}

impl Budget {
    fn new(limits: UnpackLimits) -> Self {
        Self {
            limits,
            files: 0,
            kept_bytes: 0,
            scanned_bytes: 0,
            entries: 0,
        }
    }

    fn entry(&mut self) -> Result<()> {
        self.entries += 1;

        if self.entries > MAX_ENTRIES {
            return Err(Error::payload_too_large(format!(
                "the package holds more than {MAX_ENTRIES} entries"
            )));
        }

        Ok(())
    }

    fn keep(&mut self, path: &str, size: u64) -> Result<()> {
        if size > MAX_FILE_BYTES {
            return Err(Error::payload_too_large(format!(
                "`{path}` is larger than {} MiB",
                MAX_FILE_BYTES / 1024 / 1024
            )));
        }

        self.files += 1;
        self.kept_bytes += size;

        if self.files > self.limits.max_files {
            return Err(Error::payload_too_large(format!(
                "the package holds more than {} files",
                self.limits.max_files
            )));
        }

        if self.kept_bytes > self.limits.max_bytes {
            return Err(Error::payload_too_large(format!(
                "the package unpacks to more than {} MiB",
                self.limits.max_bytes / 1024 / 1024
            )));
        }

        Ok(())
    }

    fn scanned(&mut self, size: u64) -> Result<()> {
        self.scanned_bytes += size;

        if self.scanned_bytes > MAX_SCANNED_BYTES {
            return Err(Error::payload_too_large(
                "the package unpacks to more than the server reads",
            ));
        }

        Ok(())
    }
}

/// 读一个文件，最多读到上限加一个字节，读多了就是声明的大小不实。
fn read_limited(reader: impl Read, path: &str) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();

    reader
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| Error::invalid(format!("`{path}` could not be read: {error}")))?;

    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(Error::payload_too_large(format!(
            "`{path}` is larger than its header says"
        )));
    }

    Ok(bytes)
}

/// 包里有哪些普通文件（安全的相对路径），不读内容；链接与要略过的条目不列。
pub fn list_paths(bytes: &[u8], kind: ArchiveKind) -> Result<Vec<String>> {
    let mut paths = Vec::new();

    let listing = UnpackLimits {
        max_files: usize::MAX,
        max_bytes: u64::MAX,
        skip_links: true,
    };

    unpack_with(bytes, kind, listing, |path| {
        paths.push(path.to_string());
        false
    })?;

    Ok(paths)
}

/// 解开包，`keep` 决定一个文件要不要留下。
pub fn unpack(
    bytes: &[u8],
    kind: ArchiveKind,
    limits: UnpackLimits,
    keep: impl Fn(&str) -> bool,
) -> Result<Unpacked> {
    unpack_with(bytes, kind, limits, keep)
}

fn unpack_with(
    bytes: &[u8],
    kind: ArchiveKind,
    limits: UnpackLimits,
    keep: impl FnMut(&str) -> bool,
) -> Result<Unpacked> {
    match kind {
        ArchiveKind::Zip => unpack_zip(bytes, limits, keep),
        ArchiveKind::TarGz => unpack_tar_gz(bytes, limits, keep),
    }
}

fn link_found(limits: UnpackLimits, raw: &str) -> Result<()> {
    if limits.skip_links {
        Ok(())
    } else {
        Err(Error::invalid(format!(
            "the package holds `{raw}`, which is a link or a special file"
        )))
    }
}

fn unpack_zip(
    bytes: &[u8],
    limits: UnpackLimits,
    mut keep: impl FnMut(&str) -> bool,
) -> Result<Unpacked> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| Error::invalid(format!("not a zip package: {error}")))?;

    let mut budget = Budget::new(limits);
    let mut files = Vec::new();

    for index in 0..archive.len() {
        budget.entry()?;

        let mut file = archive
            .by_index(index)
            .map_err(|error| Error::invalid(format!("zip entry {index}: {error}")))?;

        if file.is_dir() {
            continue;
        }

        let raw = file.name().to_string();

        if file
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            link_found(limits, &raw)?;
            continue;
        }

        if file.encrypted() {
            return Err(Error::invalid(format!("`{raw}` is encrypted")));
        }

        let Some(path) = safe_path(&raw)? else {
            continue;
        };

        if !keep(&path) {
            continue;
        }

        budget.keep(&path, file.size())?;
        let content = read_limited(&mut file, &path)?;
        budget.scanned(content.len() as u64)?;

        files.push(SkillFile {
            path,
            bytes: content,
        });
    }

    Ok(Unpacked {
        files,
        commit: None,
    })
}

fn unpack_tar_gz(
    bytes: &[u8],
    limits: UnpackLimits,
    mut keep: impl FnMut(&str) -> bool,
) -> Result<Unpacked> {
    let mut archive = tar::Archive::new(GzDecoder::new(Cursor::new(bytes)));
    let mut budget = Budget::new(limits);
    let mut unpacked = Unpacked::default();

    let entries = archive
        .entries()
        .map_err(|error| Error::invalid(format!("not a .tar.gz package: {error}")))?;

    for entry in entries {
        budget.entry()?;

        let mut entry = entry.map_err(|error| Error::invalid(format!("tar entry: {error}")))?;

        let kind = entry.header().entry_type();

        if kind.is_pax_global_extensions() {
            // `git archive` 把提交写在 pax 全局头里：`comment=<sha>`。
            let mut header = String::new();
            let _ = (&mut entry).take(64 * 1024).read_to_string(&mut header);

            unpacked.commit = header
                .lines()
                .filter_map(|line| line.split_once(" comment="))
                .map(|(_, value)| value.trim().to_string())
                .find(|value| !value.is_empty());

            continue;
        }

        if kind.is_dir() || kind.is_pax_local_extensions() || kind.is_gnu_longname() {
            continue;
        }

        let raw = entry
            .path()
            .map_err(|error| Error::invalid(format!("tar entry path: {error}")))?
            .to_string_lossy()
            .to_string();

        if !kind.is_file() {
            link_found(limits, &raw)?;
            continue;
        }

        let size = entry.size();
        budget.scanned(size)?;

        let Some(path) = safe_path(&raw)? else {
            continue;
        };

        if !keep(&path) {
            continue;
        }

        budget.keep(&path, size)?;
        let content = read_limited(&mut entry, &path)?;

        unpacked.files.push(SkillFile {
            path,
            bytes: content,
        });
    }

    Ok(unpacked)
}

/// 把一个 Skill 的文件打成 zip，放在以 `root` 命名的目录里，下载用。
pub fn zip_files(root: &str, files: &[SkillFile]) -> Result<Vec<u8>> {
    use std::io::Write;

    let mut buffer = Cursor::new(Vec::new());

    {
        let mut writer = zip::ZipWriter::new(&mut buffer);

        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        for file in files {
            writer
                .start_file(format!("{root}/{}", file.path), options)
                .map_err(|error| Error::internal(format!("skill zip: {error}")))?;

            writer
                .write_all(&file.bytes)
                .map_err(|error| Error::internal(format!("skill zip: {error}")))?;
        }

        writer
            .finish()
            .map_err(|error| Error::internal(format!("skill zip: {error}")))?;
    }

    Ok(buffer.into_inner())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::{Compression, write::GzEncoder};

    use super::*;

    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buffer = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buffer);

            for (path, content) in entries {
                writer
                    .start_file(*path, zip::write::SimpleFileOptions::default())
                    .unwrap();
                writer.write_all(content).unwrap();
            }

            writer.finish().unwrap();
        }
        buffer.into_inner()
    }

    fn tar_gz_of(entries: &[(&str, &[u8])], comment: Option<&str>) -> Vec<u8> {
        let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));

        if let Some(comment) = comment {
            let record = format!(" comment={comment}\n");
            let record = format!("{} {record}", record.len() + 3);
            let mut header = tar::Header::new_ustar();
            header.set_entry_type(tar::EntryType::XGlobalHeader);
            header.set_size(record.len() as u64);
            header.set_cksum();
            builder
                .append_data(&mut header, "pax_global_header", record.as_bytes())
                .unwrap();
        }

        for (path, content) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, path, *content).unwrap();
        }

        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn reads_both_formats_and_skips_finder_droppings() {
        let zip = zip_of(&[
            ("triage/SKILL.md", b"---\n"),
            ("__MACOSX/triage/._SKILL.md", b"x"),
            ("triage/.DS_Store", b"x"),
        ]);
        let files = unpack(&zip, ArchiveKind::Zip, PACKAGE_LIMITS, |_| true)
            .unwrap()
            .files;
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "triage/SKILL.md");

        let tgz = tar_gz_of(&[("repo-abc/skills/a/SKILL.md", b"---\n")], Some("abc123"));
        let unpacked = unpack(&tgz, ArchiveKind::TarGz, PACKAGE_LIMITS, |_| true).unwrap();
        assert_eq!(unpacked.files[0].path, "repo-abc/skills/a/SKILL.md");
        assert_eq!(unpacked.commit.as_deref(), Some("abc123"));
    }

    #[test]
    fn keeps_only_what_is_asked_for() {
        let tgz = tar_gz_of(&[("r/a/SKILL.md", b"a"), ("r/README.md", b"b")], None);
        let files = unpack(&tgz, ArchiveKind::TarGz, PACKAGE_LIMITS, |path| {
            path.starts_with("r/a/")
        })
        .unwrap()
        .files;
        assert_eq!(files.len(), 1);
        assert_eq!(
            list_paths(&tgz, ArchiveKind::TarGz).unwrap(),
            vec!["r/a/SKILL.md", "r/README.md"]
        );
    }

    #[test]
    fn refuses_paths_that_leave_the_package() {
        assert!(safe_path("../etc/passwd").is_err());
        assert!(safe_path("a/../../b").is_err());
        assert!(safe_path("/etc/passwd").is_err());
        assert!(safe_path("C:\\\\windows").is_err());
        assert_eq!(safe_path("a\\\\b.md").unwrap().as_deref(), Some("a/b.md"));
        assert_eq!(safe_path("./a//b.md").unwrap().as_deref(), Some("a/b.md"));
        assert_eq!(safe_path("__MACOSX/x").unwrap(), None);
    }

    #[test]
    fn refuses_links_in_a_tarball() {
        let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_cksum();
        builder
            .append_link(&mut header, "skill/evil", "/etc/passwd")
            .unwrap();
        let bytes = builder.into_inner().unwrap().finish().unwrap();

        assert!(unpack(&bytes, ArchiveKind::TarGz, PACKAGE_LIMITS, |_| true).is_err());

        // A repository may hold links; they are passed over, not followed.
        let unpacked = unpack(&bytes, ArchiveKind::TarGz, REPOSITORY_LIMITS, |_| true).unwrap();
        assert!(unpacked.files.is_empty());
    }

    #[test]
    fn caps_the_number_of_files() {
        let names = (0..=MAX_FILES)
            .map(|index| format!("s/{index}.md"))
            .collect::<Vec<_>>();
        let entries = names
            .iter()
            .map(|name| (name.as_str(), b"x".as_slice()))
            .collect::<Vec<_>>();

        assert!(
            unpack(&zip_of(&entries), ArchiveKind::Zip, PACKAGE_LIMITS, |_| {
                true
            })
            .is_err()
        );
    }

    #[test]
    fn knows_the_formats_by_name() {
        assert_eq!(archive_kind("triage.skill"), Some(ArchiveKind::Zip));
        assert_eq!(archive_kind("A.ZIP"), Some(ArchiveKind::Zip));
        assert_eq!(archive_kind("x.tar.gz"), Some(ArchiveKind::TarGz));
        assert_eq!(archive_kind("x.tgz"), Some(ArchiveKind::TarGz));
        assert_eq!(archive_kind("x.rar"), None);
    }
}
