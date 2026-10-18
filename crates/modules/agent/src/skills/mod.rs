// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Agent Skills：一个目录里的 `SKILL.md`（YAML frontmatter + 正文）加任意参考文件，兼容开放的
//! Agent Skills 格式。这里是与来源无关的部分：读清单、解包、找出包或仓库里的 Skill、算内容
//! hash、填参数、渲染给模型看的清单。来源（内置、实例目录、上传、仓库）与存储在应用层。

mod archive;
mod catalog;
mod manifest;
mod repository;
mod store;
mod template;

use std::collections::BTreeSet;

pub use archive::{
    ArchiveKind, PACKAGE_LIMITS, REPOSITORY_LIMITS, UnpackLimits, Unpacked, archive_kind,
    list_paths, safe_path, unpack, zip_files,
};
pub use catalog::{
    CatalogEntry, MAX_CATALOG_CHARS, MAX_CATALOG_ENTRIES, SKILL_SAFETY_NOTE, render_catalog,
    wrap_skill_body,
};
pub use manifest::{MAX_DESCRIPTION_CHARS, SkillManifest, parse_skill_md, validate_name};
pub use repository::{
    ArchiveRequest, RepoHost, archive_request, normalize_repo_dir, parse_repo_url,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
pub use store::{
    RepositoryStatus, SkillRepository, SkillSource, SkillStore, StoredSkill, TokenChange,
};
pub use template::render_invocation;

use crate::shared::{Error, Result};

/// 每个 Skill 根目录下的入口文件。
pub const SKILL_FILE: &str = "SKILL.md";

/// 上传的包最大多少字节。
pub const MAX_PACKAGE_BYTES: usize = 20 * 1024 * 1024;

/// 从仓库下载的压缩包最大多少字节。
pub const MAX_REPOSITORY_ARCHIVE_BYTES: usize = 64 * 1024 * 1024;

/// 一个 Skill 最多多少个文件。
pub const MAX_FILES: usize = 300;

/// 单个文件最大多少字节。
pub const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

/// 一个 Skill 解开后最多多少字节。
pub const MAX_UNPACKED_BYTES: u64 = 20 * 1024 * 1024;

/// 一个仓库最多导入多少个 Skill。
pub const MAX_SKILLS_PER_REPOSITORY: usize = 50;

/// 预览或交给模型读的文本文件最多多少字节。
pub const MAX_TEXT_FILE_BYTES: usize = 256 * 1024;

/// Skill 里的一个文件：相对 Skill 根目录的路径与内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillFile {
    pub path: String,
    pub bytes: Vec<u8>,
}

/// 文件清单里的一项。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
    /// 能当文本预览、交给模型读。
    pub text: bool,
}

/// 内容是 UTF-8 文本且不太大时当文本。
pub fn is_text(bytes: &[u8]) -> bool {
    bytes.len() <= MAX_TEXT_FILE_BYTES && !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok()
}

/// 一个 Skill 的全部内容。
#[derive(Debug, Clone, PartialEq)]
pub struct SkillBundle {
    pub manifest: SkillManifest,
    /// `SKILL.md` 去掉 frontmatter 的正文。
    pub body: String,
    /// 按路径排好序，含 `SKILL.md`。
    pub files: Vec<SkillFile>,
    /// 全部文件的 sha256：路径、长度与内容依次参与。
    pub hash: String,
}

impl SkillBundle {
    /// 由 Skill 根目录下的文件组成，根目录下必须有 `SKILL.md`。
    pub fn from_files(mut files: Vec<SkillFile>) -> Result<Self> {
        files.sort_by(|left, right| left.path.cmp(&right.path));

        if files.len() > MAX_FILES {
            return Err(Error::payload_too_large(format!(
                "a skill holds at most {MAX_FILES} files"
            )));
        }

        let total: u64 = files.iter().map(|file| file.bytes.len() as u64).sum();

        if total > MAX_UNPACKED_BYTES {
            return Err(Error::payload_too_large(format!(
                "a skill holds at most {} MiB",
                MAX_UNPACKED_BYTES / 1024 / 1024
            )));
        }

        let entry = files
            .iter()
            .find(|file| file.path == SKILL_FILE)
            .ok_or_else(|| Error::invalid("the skill has no SKILL.md at its root"))?;

        let text = std::str::from_utf8(&entry.bytes)
            .map_err(|_| Error::invalid("SKILL.md is not UTF-8 text"))?;

        let (manifest, body) = parse_skill_md(text)?;
        let hash = content_hash(&files);

        Ok(Self {
            manifest,
            body,
            files,
            hash,
        })
    }

    pub fn index(&self) -> Vec<FileEntry> {
        self.files
            .iter()
            .map(|file| FileEntry {
                path: file.path.clone(),
                size: file.bytes.len() as u64,
                text: is_text(&file.bytes),
            })
            .collect()
    }

    pub fn file(&self, path: &str) -> Option<&SkillFile> {
        self.files.iter().find(|file| file.path == path)
    }
}

pub fn content_hash(files: &[SkillFile]) -> String {
    let mut sorted: Vec<&SkillFile> = files.iter().collect();
    sorted.sort_by(|left, right| left.path.cmp(&right.path));

    let mut hasher = Sha256::new();

    for file in sorted {
        hasher.update(file.path.as_bytes());
        hasher.update([0]);
        hasher.update((file.bytes.len() as u64).to_be_bytes());
        hasher.update(&file.bytes);
    }

    hex::encode(hasher.finalize())
}

/// 去掉一层前缀后的路径；不在这一层下时为空。
fn strip_dir<'a>(path: &'a str, dir: &str) -> Option<&'a str> {
    if dir.is_empty() {
        return Some(path);
    }

    path.strip_prefix(dir)?.strip_prefix('/')
}

/// 上传的包：`SKILL.md` 在包的根，或在包唯一的顶层目录里。
pub fn bundle_from_package(files: Vec<SkillFile>) -> Result<SkillBundle> {
    if files.iter().any(|file| file.path == SKILL_FILE) {
        return SkillBundle::from_files(files);
    }

    let tops: BTreeSet<&str> = files
        .iter()
        .filter_map(|file| file.path.split_once('/').map(|(top, _)| top))
        .collect();

    let single_top = (tops.len() == 1 && files.iter().all(|file| file.path.contains('/')))
        .then(|| tops.into_iter().next())
        .flatten()
        .map(str::to_string);

    match single_top {
        Some(top)
            if files
                .iter()
                .any(|file| file.path == format!("{top}/{SKILL_FILE}")) =>
        {
            let files = files
                .into_iter()
                .filter_map(|file| {
                    strip_dir(&file.path, &top).map(|path| SkillFile {
                        path: path.to_string(),
                        bytes: file.bytes.clone(),
                    })
                })
                .collect();

            SkillBundle::from_files(files)
        }
        _ => Err(Error::invalid(
            "SKILL.md must be at the root of the package or in its only folder",
        )),
    }
}

/// 仓库压缩包的唯一顶层目录（托管站打包时加的 `仓库名-提交/`）。
pub fn archive_top_dir(paths: &[String]) -> Option<String> {
    let tops: BTreeSet<&str> = paths
        .iter()
        .map(|path| path.split_once('/').map_or(path.as_str(), |(top, _)| top))
        .collect();

    (tops.len() == 1 && paths.iter().all(|path| path.contains('/')))
        .then(|| tops.into_iter().next().map(str::to_string))
        .flatten()
}

/// `base` 目录下的 Skill 目录（相对仓库根）：含 `SKILL.md` 的目录，嵌在另一个 Skill 目录里的
/// 不算。`base` 本身是 Skill 时只有它一个。
pub fn skill_dirs(paths: &[String], base: &str) -> Vec<String> {
    let mut dirs: Vec<String> = paths
        .iter()
        .filter_map(|path| strip_dir(path, base))
        .filter_map(|rest| {
            if rest == SKILL_FILE {
                Some(String::new())
            } else {
                rest.strip_suffix(&format!("/{SKILL_FILE}"))
                    .map(str::to_string)
            }
        })
        .collect();

    dirs.sort();
    dirs.dedup();

    let mut outermost: Vec<String> = Vec::new();

    for dir in dirs {
        let nested = outermost
            .iter()
            .any(|outer| outer.is_empty() || dir.starts_with(&format!("{outer}/")));

        if !nested {
            outermost.push(dir);
        }
    }

    outermost
        .into_iter()
        .map(|dir| match (base.is_empty(), dir.is_empty()) {
            (true, _) => dir,
            (false, true) => base.to_string(),
            (false, false) => format!("{base}/{dir}"),
        })
        .collect()
}

/// 某个 Skill 目录下的文件，路径改成相对这个目录。
pub fn files_under(files: &[SkillFile], dir: &str) -> Vec<SkillFile> {
    files
        .iter()
        .filter_map(|file| {
            strip_dir(&file.path, dir).map(|path| SkillFile {
                path: path.to_string(),
                bytes: file.bytes.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, content: &str) -> SkillFile {
        SkillFile {
            path: path.into(),
            bytes: content.as_bytes().to_vec(),
        }
    }

    const SKILL: &str = "---\nname: triage\ndescription: Triage.\n---\nBody\n";

    #[test]
    fn a_package_has_its_skill_at_the_root_or_in_its_only_folder() {
        let at_root =
            bundle_from_package(vec![file("SKILL.md", SKILL), file("ref/a.md", "a")]).unwrap();
        assert_eq!(at_root.manifest.name, "triage");
        assert_eq!(at_root.files.len(), 2);

        let in_folder = bundle_from_package(vec![
            file("triage/SKILL.md", SKILL),
            file("triage/b.md", "b"),
        ])
        .unwrap();
        assert_eq!(in_folder.index()[1].path, "b.md");

        assert!(bundle_from_package(vec![file("a/SKILL.md", SKILL), file("b/x.md", "x")]).is_err());
        assert!(bundle_from_package(vec![file("README.md", "x")]).is_err());
    }

    #[test]
    fn the_hash_follows_the_content_not_the_order() {
        let one = content_hash(&[file("a", "1"), file("b", "2")]);
        let two = content_hash(&[file("b", "2"), file("a", "1")]);
        let changed = content_hash(&[file("a", "1"), file("b", "3")]);

        assert_eq!(one, two);
        assert_ne!(one, changed);
        assert_eq!(one.len(), 64);
    }

    #[test]
    fn finds_the_skills_of_a_repository_without_nested_ones() {
        let paths = [
            "repo-abc/README.md",
            "repo-abc/skills/triage/SKILL.md",
            "repo-abc/skills/triage/examples/SKILL.md",
            "repo-abc/skills/latency/SKILL.md",
            "repo-abc/docs/SKILL.md",
        ]
        .map(String::from)
        .to_vec();

        assert_eq!(archive_top_dir(&paths).as_deref(), Some("repo-abc"));
        assert_eq!(
            skill_dirs(&paths, "repo-abc/skills"),
            vec!["repo-abc/skills/latency", "repo-abc/skills/triage"]
        );
        assert_eq!(
            skill_dirs(&paths, "repo-abc/skills/triage"),
            vec!["repo-abc/skills/triage"]
        );
        assert_eq!(skill_dirs(&paths, "repo-abc").len(), 3);
    }

    #[test]
    fn text_files_are_utf8_without_nul() {
        assert!(is_text(b"hello"));
        assert!(!is_text(b"\0binary"));
        assert!(!is_text(&[0xff, 0xfe]));
    }
}
