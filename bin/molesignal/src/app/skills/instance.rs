// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 实例级 Skill：随产品发布的内置 Skill，与实例配置的服务器目录（`[agent] skill_paths`）里
//! 的 Skill。所有组织可见、只读，组织只能启停。
//!
//! 目录里每个含 `SKILL.md` 的子目录是一个 Skill（目录本身含 `SKILL.md` 时它就是一个）。
//! 跟随符号链接（Kubernetes 的 ConfigMap、git-sync 都靠链接），略过以 `.` 开头的条目。
//! 启动时扫描一次，之后按文件的修改时间判断要不要重扫，也可以手动重扫。

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use parking_lot::RwLock;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{
    agent::{
        commands::find_builtin,
        skills::{MAX_FILE_BYTES, MAX_FILES, SKILL_FILE, SkillBundle, SkillFile, SkillSource},
    },
    shared::time::TimestampMicros,
};

/// 一个实例级 Skill。
#[derive(Debug)]
pub struct InstanceSkill {
    /// `builtin:<名称>` 或 `path:<名称>`：组织启停按它记。
    pub key: String,
    pub source: SkillSource,
    /// 实例目录来源的目录。
    pub dir: Option<String>,
    pub bundle: SkillBundle,
}

/// 目录里没能读成 Skill 的地方。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PathProblem {
    pub dir: String,
    pub message: String,
}

#[derive(Debug, Default)]
struct PathScan {
    skills: Vec<Arc<InstanceSkill>>,
    problems: Vec<PathProblem>,
    fingerprint: String,
    scanned_at: Option<TimestampMicros>,
}

/// 内置 Skill 的 `SKILL.md`，编进二进制。
const BUILTIN: &[(&str, &str)] = &[
    (
        "incident-triage",
        include_str!("builtin/incident-triage/SKILL.md"),
    ),
    (
        "log-error-spike",
        include_str!("builtin/log-error-spike/SKILL.md"),
    ),
    (
        "latency-regression",
        include_str!("builtin/latency-regression/SKILL.md"),
    ),
    (
        "alert-noise-reduction",
        include_str!("builtin/alert-noise-reduction/SKILL.md"),
    ),
    ("init", include_str!("builtin/init/SKILL.md")),
];

fn builtin_skills() -> Vec<Arc<InstanceSkill>> {
    BUILTIN
        .iter()
        .map(|(name, text)| {
            let bundle = SkillBundle::from_files(vec![SkillFile {
                path: SKILL_FILE.into(),
                bytes: text.as_bytes().to_vec(),
            }])
            .unwrap_or_else(|error| panic!("built-in skill `{name}` is invalid: {error}"));

            Arc::new(InstanceSkill {
                key: format!("builtin:{}", bundle.manifest.name),
                source: SkillSource::Builtin,
                dir: None,
                bundle,
            })
        })
        .collect()
}

pub struct InstanceSkills {
    builtin: Vec<Arc<InstanceSkill>>,
    roots: Vec<PathBuf>,
    paths: RwLock<PathScan>,
}

impl InstanceSkills {
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self {
            builtin: builtin_skills(),
            roots,
            paths: RwLock::new(PathScan::default()),
        }
    }

    pub fn roots(&self) -> Vec<String> {
        self.roots
            .iter()
            .map(|root| root.display().to_string())
            .collect()
    }

    /// 内置在前，实例目录在后；与内置同名的目录 Skill 不收。
    pub fn all(&self) -> Vec<Arc<InstanceSkill>> {
        let mut skills = self.builtin.clone();
        skills.extend(self.paths.read().skills.iter().cloned());
        skills
    }

    pub fn find(&self, key: &str) -> Option<Arc<InstanceSkill>> {
        self.all().into_iter().find(|skill| skill.key == key)
    }

    pub fn problems(&self) -> Vec<PathProblem> {
        self.paths.read().problems.clone()
    }

    pub fn scanned_at(&self) -> Option<TimestampMicros> {
        self.paths.read().scanned_at
    }

    /// 目录有变化（按文件的路径、大小、修改时间）时重扫；`force` 时总是重扫。阻塞，放在
    /// `spawn_blocking` 里调。返回是否重扫了。
    pub fn rescan(&self, force: bool) -> bool {
        let fingerprint = fingerprint(&self.roots);

        {
            let current = self.paths.read();

            if !force && current.scanned_at.is_some() && current.fingerprint == fingerprint {
                return false;
            }
        }

        let builtin_names: BTreeSet<String> = self
            .builtin
            .iter()
            .map(|skill| skill.bundle.manifest.name.to_ascii_lowercase())
            .collect();

        let mut scan = PathScan {
            fingerprint,
            scanned_at: Some(TimestampMicros::now()),
            ..PathScan::default()
        };

        let mut names = builtin_names;

        for root in &self.roots {
            for dir in skill_dirs(root, &mut scan.problems) {
                let shown = dir.display().to_string();

                match read_skill(&dir) {
                    Ok(bundle) => {
                        let name = bundle.manifest.name.to_ascii_lowercase();

                        if find_builtin(&name).is_some() {
                            scan.problems.push(PathProblem {
                                dir: shown,
                                message: format!(
                                    "`{}` is a built-in command; rename the skill",
                                    bundle.manifest.name
                                ),
                            });
                            continue;
                        }

                        if !names.insert(name) {
                            scan.problems.push(PathProblem {
                                dir: shown,
                                message: format!(
                                    "another skill is already named `{}`",
                                    bundle.manifest.name
                                ),
                            });
                            continue;
                        }

                        scan.skills.push(Arc::new(InstanceSkill {
                            key: format!("path:{}", bundle.manifest.name),
                            source: SkillSource::Path,
                            dir: Some(shown),
                            bundle,
                        }));
                    }
                    Err(message) => scan.problems.push(PathProblem {
                        dir: shown,
                        message,
                    }),
                }
            }
        }

        *self.paths.write() = scan;
        true
    }
}

fn hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('.'))
}

/// 根目录下的 Skill 目录：根自己含 `SKILL.md` 时只有它。
fn skill_dirs(root: &Path, problems: &mut Vec<PathProblem>) -> Vec<PathBuf> {
    if root.join(SKILL_FILE).is_file() {
        return vec![root.to_path_buf()];
    }

    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) => {
            problems.push(PathProblem {
                dir: root.display().to_string(),
                message: format!("the folder cannot be read: {error}"),
            });
            return Vec::new();
        }
    };

    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| !hidden(path) && path.is_dir() && path.join(SKILL_FILE).is_file())
        .collect();

    dirs.sort();
    dirs
}

/// 读一个 Skill 目录的全部文件，有数量、大小与深度上限。
fn read_skill(dir: &Path) -> std::result::Result<SkillBundle, String> {
    let mut files = Vec::new();
    collect(dir, dir, 0, &mut files)?;

    SkillBundle::from_files(files).map_err(|error| error.to_string())
}

fn collect(
    root: &Path,
    dir: &Path,
    depth: usize,
    files: &mut Vec<SkillFile>,
) -> std::result::Result<(), String> {
    if depth > 8 {
        return Err("the skill folder is nested too deep".into());
    }

    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|error| format!("`{}` cannot be read: {error}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| !hidden(path))
        .collect();

    entries.sort();

    for path in entries {
        let metadata = fs::metadata(&path)
            .map_err(|error| format!("`{}` cannot be read: {error}", path.display()))?;

        if metadata.is_dir() {
            collect(root, &path, depth + 1, files)?;
            continue;
        }

        if !metadata.is_file() {
            continue;
        }

        if metadata.len() > MAX_FILE_BYTES {
            return Err(format!("`{}` is too large", path.display()));
        }

        if files.len() >= MAX_FILES {
            return Err(format!("a skill holds at most {MAX_FILES} files"));
        }

        let relative = path
            .strip_prefix(root)
            .map_err(|_| "a file lies outside the skill folder".to_string())?
            .components()
            .map(|part| part.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("/");

        let bytes = fs::read(&path)
            .map_err(|error| format!("`{}` cannot be read: {error}", path.display()))?;

        files.push(SkillFile {
            path: relative,
            bytes,
        });
    }

    Ok(())
}

/// 目录里全部文件的路径、大小与修改时间，用来判断要不要重扫。
fn fingerprint(roots: &[PathBuf]) -> String {
    fn walk(dir: &Path, depth: usize, hasher: &mut Sha256) {
        if depth > 10 {
            return;
        }

        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };

        let mut paths: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| !hidden(path))
            .collect();

        paths.sort();

        for path in paths {
            let Ok(metadata) = fs::metadata(&path) else {
                continue;
            };

            if metadata.is_dir() {
                walk(&path, depth + 1, hasher);
                continue;
            }

            let modified = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_nanos())
                .unwrap_or_default();

            hasher.update(path.to_string_lossy().as_bytes());
            hasher.update(metadata.len().to_be_bytes());
            hasher.update(modified.to_be_bytes());
        }
    }

    let mut hasher = Sha256::new();

    for root in roots {
        hasher.update(root.to_string_lossy().as_bytes());
        walk(root, 0, &mut hasher);
    }

    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("molesignal-skills-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn the_built_in_skills_are_valid_and_named_after_their_folders() {
        let skills = builtin_skills();

        assert_eq!(skills.len(), BUILTIN.len());

        for (skill, (name, _)) in skills.iter().zip(BUILTIN) {
            assert_eq!(skill.bundle.manifest.name, *name);
            assert!(!skill.bundle.manifest.allowed_tools.is_empty());
        }
    }

    #[test]
    fn every_built_in_skill_is_written_in_both_languages() {
        use crate::{
            app::skills::{BUNDLED_SKILL_KEY, bundled_text},
            shared::i18n_language_seed::seeded_pack,
        };

        for language in ["en-us", "zh-cn"] {
            let texts = seeded_pack(BUNDLED_SKILL_KEY, language);

            for skill in builtin_skills() {
                let manifest = &skill.bundle.manifest;
                let mut parts = vec!["title", "description"];

                if manifest.argument_hint.is_some() {
                    parts.push("argument_hint");
                }

                for part in parts {
                    assert!(
                        bundled_text(&texts, &manifest.name, part).is_some(),
                        "`{}` has no {part} in {language}",
                        manifest.name
                    );
                }
            }
        }
    }

    #[test]
    fn a_folder_of_skills_is_read_and_rescanned_when_it_changes() {
        let root = scratch("scan");
        write(
            &root.join("runbook/SKILL.md"),
            "---\nname: runbook\ndescription: Follow the runbook.\n---\nSteps",
        );
        write(&root.join("runbook/refs/a.md"), "ref");
        write(&root.join("broken/SKILL.md"), "no frontmatter");
        write(
            &root.join("dup/SKILL.md"),
            "---\nname: incident-triage\ndescription: x\n---\n",
        );
        write(
            &root.join(".hidden/SKILL.md"),
            "---\nname: h\ndescription: h\n---\n",
        );

        let instance = InstanceSkills::new(vec![root.clone()]);
        assert!(instance.rescan(false));

        let path_skills: Vec<_> = instance
            .all()
            .into_iter()
            .filter(|skill| skill.source == SkillSource::Path)
            .collect();
        assert_eq!(path_skills.len(), 1);
        assert_eq!(path_skills[0].key, "path:runbook");
        assert_eq!(path_skills[0].bundle.files.len(), 2);
        assert_eq!(instance.problems().len(), 2);

        assert!(!instance.rescan(false));
        write(&root.join("runbook/refs/b.md"), "more");
        assert!(instance.rescan(false));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_unreadable_root_is_a_problem_not_a_failure() {
        let instance = InstanceSkills::new(vec![PathBuf::from("/nonexistent/molesignal/skills")]);
        assert!(instance.rescan(true));
        assert_eq!(instance.problems().len(), 1);
        assert_eq!(instance.all().len(), BUILTIN.len());
    }
}
