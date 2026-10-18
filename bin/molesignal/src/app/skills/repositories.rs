// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Skill 仓库：按分支或标签下载压缩包，仓库（或其中一个目录）下每个含 `SKILL.md` 的目录是一个
//! Skill。同步时新增、更新、移除；格式不对或与同一作用域里别的 Skill 重名的不导入，记在同步
//! 结果里。仓库移到别的作用域时，它的 Skill 跟着移。
//!
//! 管理员建的组织级仓库可以连内网；团队成员与个人建的只能连公网，组织网络策略里允许的
//! 内网主机与网段除外。

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use super::{SkillService, fetch, install::check_name};
use crate::{
    agent::{
        scope::{Scope, Viewer},
        skills::{
            ArchiveKind, MAX_REPOSITORY_ARCHIVE_BYTES, MAX_SKILLS_PER_REPOSITORY,
            REPOSITORY_LIMITS, RepoHost, RepositoryStatus, SkillBundle, SkillRepository,
            SkillSource, StoredSkill, TokenChange, archive_request, archive_top_dir, files_under,
            list_paths, normalize_repo_dir, parse_repo_url, skill_dirs, unpack,
        },
    },
    app::egress::TargetPolicy,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 自动同步的间隔。
const AUTO_SYNC_EVERY_MICROS: i64 = 24 * 3_600 * 1_000_000;

/// 新建或修改仓库时填的内容。
pub struct RepositoryInput {
    pub scope: Scope,
    pub url: String,
    /// 不填时由地址认出；自建的托管站要选。
    pub host: Option<RepoHost>,
    pub archive_url: Option<String>,
    pub git_ref: String,
    pub dir: String,
    pub auto_sync: bool,
    pub token: TokenChange,
}

/// 一次同步的结果。
#[derive(Debug, Clone, Serialize)]
pub struct SyncReport {
    pub status: RepositoryStatus,
    pub imported: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub removed: usize,
    /// 没导入的目录与原因，或整体失败的原因。
    pub problems: Vec<String>,
    pub commit: Option<String>,
}

impl SyncReport {
    fn failed(error: &Error) -> Self {
        Self {
            status: RepositoryStatus::Failed,
            imported: 0,
            updated: 0,
            unchanged: 0,
            removed: 0,
            problems: vec![error.to_string()],
            commit: None,
        }
    }
}

/// 仓库根下的路径：拼上压缩包的顶层目录。
fn under_top(top: &str, dir: &str) -> String {
    match (top.is_empty(), dir.is_empty()) {
        (true, _) => dir.to_string(),
        (false, true) => top.to_string(),
        (false, false) => format!("{top}/{dir}"),
    }
}

fn relative_to_top<'a>(top: &str, path: &'a str) -> &'a str {
    if top.is_empty() {
        return path;
    }

    path.strip_prefix(top)
        .map(|rest| rest.trim_start_matches('/'))
        .unwrap_or(path)
}

impl SkillService {
    pub async fn list_repositories(
        &self,
        org_id: &Id,
        viewer: &Viewer,
    ) -> Result<Vec<SkillRepository>> {
        Ok(self
            .store()
            .list_repositories(org_id)
            .await?
            .into_iter()
            .filter(|repository| viewer.can_see(&repository.scope))
            .collect())
    }

    /// 校验填的内容，套到一个仓库记录上。
    fn apply_input(&self, repository: &mut SkillRepository, input: &RepositoryInput) -> Result<()> {
        let url = parse_repo_url(&input.url)?;

        let host = input
            .host
            .or_else(|| RepoHost::detect(&url))
            .ok_or_else(|| {
                Error::invalid("choose how the repository is hosted: GitHub, GitLab, Gitea or Bitbucket, or give its archive address")
            })?;

        let git_ref = input.git_ref.trim().to_string();

        if git_ref.chars().count() > 255 || git_ref.contains(char::is_whitespace) {
            return Err(Error::invalid("the branch or tag is not valid"));
        }

        let archive_url = input
            .archive_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);

        // 地址与分支能拼出下载地址，才算填对。
        archive_request(host, &url, &git_ref, archive_url.as_deref())?;

        repository.scope = input.scope.clone();
        repository.url = url.to_string();
        repository.host = host;
        repository.archive_url = archive_url;
        repository.git_ref = git_ref;
        repository.dir = normalize_repo_dir(&input.dir)?;
        repository.auto_sync = input.auto_sync;
        Ok(())
    }

    pub async fn add_repository(
        &self,
        org_id: &Id,
        viewer: &Viewer,
        input: RepositoryInput,
    ) -> Result<(SkillRepository, SyncReport)> {
        if !viewer.can_edit(&input.scope) {
            return Err(Error::forbidden("you cannot add skills to this scope"));
        }

        let now = TimestampMicros::now();

        let mut repository = SkillRepository {
            id: Id::new(),
            org_id: org_id.clone(),
            scope: input.scope.clone(),
            url: String::new(),
            host: RepoHost::Github,
            archive_url: None,
            git_ref: String::new(),
            dir: String::new(),
            auto_sync: false,
            token_set: false,
            token_last4: None,
            status: RepositoryStatus::Pending,
            last_error: None,
            last_commit: None,
            last_synced_at: None,
            created_by: viewer.user_id.clone(),
            updated_by: viewer.user_id.clone(),
            created_at: now,
            updated_at: now,
        };

        self.apply_input(&mut repository, &input)?;

        let saved = self
            .store()
            .save_repository(repository, input.token)
            .await?;
        let report = self.sync(&saved).await;
        let saved = self.store().get_repository(org_id, &saved.id).await?;

        Ok((saved, report))
    }

    pub async fn update_repository(
        &self,
        org_id: &Id,
        viewer: &Viewer,
        id: &Id,
        input: RepositoryInput,
    ) -> Result<(SkillRepository, SyncReport)> {
        let mut repository = self.store().get_repository(org_id, id).await?;

        if !viewer.can_see(&repository.scope) {
            return Err(Error::not_found(format!(
                "skill repository `{}` not found",
                id.0
            )));
        }

        if !viewer.can_edit(&repository.scope) || !viewer.can_edit(&input.scope) {
            return Err(Error::forbidden(
                "changing this repository needs permission in its scope",
            ));
        }

        self.apply_input(&mut repository, &input)?;
        repository.updated_by = viewer.user_id.clone();
        repository.updated_at = TimestampMicros::now();

        let saved = self
            .store()
            .save_repository(repository, input.token)
            .await?;
        let report = self.sync(&saved).await;
        let saved = self.store().get_repository(org_id, &saved.id).await?;

        Ok((saved, report))
    }

    pub async fn delete_repository(
        &self,
        org_id: &Id,
        viewer: &Viewer,
        id: &Id,
    ) -> Result<SkillRepository> {
        let repository = self.store().get_repository(org_id, id).await?;

        if !viewer.can_see(&repository.scope) {
            return Err(Error::not_found(format!(
                "skill repository `{}` not found",
                id.0
            )));
        }

        if !viewer.can_moderate(&repository.scope) {
            return Err(Error::forbidden("this repository belongs to someone else"));
        }

        let skills = self.repository_skills(&repository).await?;

        self.store().delete_repository(org_id, id).await?;

        for skill in skills {
            self.remove_objects(&format!("{}/", skill.object_prefix.trim_end_matches('/')))
                .await;
        }

        Ok(repository)
    }

    pub async fn sync_repository(
        &self,
        org_id: &Id,
        viewer: &Viewer,
        id: &Id,
    ) -> Result<SyncReport> {
        let repository = self.store().get_repository(org_id, id).await?;

        if !viewer.can_see(&repository.scope) {
            return Err(Error::not_found(format!(
                "skill repository `{}` not found",
                id.0
            )));
        }

        if !viewer.can_edit(&repository.scope) {
            return Err(Error::forbidden(
                "syncing this repository needs permission in its scope",
            ));
        }

        Ok(self.sync(&repository).await)
    }

    /// 自动同步到期的仓库，返回同步了几个。
    pub async fn sync_due(&self) -> usize {
        let before = TimestampMicros(TimestampMicros::now().0 - AUTO_SYNC_EVERY_MICROS);

        let repositories = match self.store().repositories_due(before).await {
            Ok(repositories) => repositories,
            Err(error) => {
                tracing::warn!(%error, "skill repositories due for sync could not be read");
                return 0;
            }
        };

        for repository in &repositories {
            let report = self.sync(repository).await;

            tracing::info!(
                repository = %repository.id.0,
                status = report.status.as_str(),
                "skill repository synced"
            );
        }

        repositories.len()
    }

    async fn repository_skills(&self, repository: &SkillRepository) -> Result<Vec<StoredSkill>> {
        Ok(self
            .store()
            .list(&repository.org_id)
            .await?
            .into_iter()
            .filter(|skill| skill.repository_id.as_ref() == Some(&repository.id))
            .collect())
    }

    async fn policy_for(&self, repository: &SkillRepository) -> Result<TargetPolicy> {
        if repository.scope == Scope::Org {
            return Ok(TargetPolicy::admin_any());
        }

        let policy = self.network().get(&repository.org_id).await?;

        Ok(TargetPolicy::member(&policy))
    }

    /// 同步并记下结果；失败不抛出，写进仓库状态。
    async fn sync(&self, repository: &SkillRepository) -> SyncReport {
        let report = match self.try_sync(repository).await {
            Ok(report) => report,
            Err(error) => SyncReport::failed(&error),
        };

        let error = (!report.problems.is_empty()).then(|| report.problems.join("\n"));

        if let Err(error) = self
            .store()
            .record_sync(
                &repository.org_id,
                &repository.id,
                report.status,
                error.as_deref(),
                report.commit.as_deref(),
            )
            .await
        {
            tracing::warn!(%error, "skill repository sync result was not recorded");
        }

        report
    }

    async fn try_sync(&self, repository: &SkillRepository) -> Result<SyncReport> {
        let url = parse_repo_url(&repository.url)?;

        let request = archive_request(
            repository.host,
            &url,
            &repository.git_ref,
            repository.archive_url.as_deref(),
        )?;

        let token = self
            .store()
            .repository_token(&repository.org_id, &repository.id)
            .await?;

        let policy = self.policy_for(repository).await?;

        let bytes = fetch::download(
            &request,
            token.as_deref(),
            &policy,
            MAX_REPOSITORY_ARCHIVE_BYTES,
        )
        .await?;

        let kind = if bytes.starts_with(b"PK") {
            ArchiveKind::Zip
        } else {
            ArchiveKind::TarGz
        };

        let paths = list_paths(&bytes, kind)?;
        let top = archive_top_dir(&paths).unwrap_or_default();
        let base = under_top(&top, &repository.dir);
        let mut dirs = skill_dirs(&paths, &base);
        let mut problems = Vec::new();

        if dirs.is_empty() {
            return Err(Error::invalid(format!(
                "no SKILL.md was found under `{}`",
                if repository.dir.is_empty() {
                    "/"
                } else {
                    &repository.dir
                }
            )));
        }

        if dirs.len() > MAX_SKILLS_PER_REPOSITORY {
            problems.push(format!(
                "only the first {MAX_SKILLS_PER_REPOSITORY} of {} skills were imported",
                dirs.len()
            ));
            dirs.truncate(MAX_SKILLS_PER_REPOSITORY);
        }

        let wanted: Vec<String> = dirs.iter().map(|dir| format!("{dir}/")).collect();

        let unpacked = unpack(&bytes, kind, REPOSITORY_LIMITS, |path| {
            dirs.iter().any(|dir| dir.is_empty())
                || wanted
                    .iter()
                    .any(|prefix| path.starts_with(prefix.as_str()))
        })?;

        let existing = self.repository_skills(repository).await?;
        let new_dirs: HashSet<String> = dirs
            .iter()
            .map(|dir| relative_to_top(&top, dir).to_string())
            .collect();

        let mut report = SyncReport {
            status: RepositoryStatus::Ok,
            imported: 0,
            updated: 0,
            unchanged: 0,
            removed: 0,
            problems: Vec::new(),
            commit: unpacked.commit.clone(),
        };

        // 先移除仓库里已经没有的目录，改名的目录才不会和旧的自己撞名。
        let mut kept: HashMap<String, StoredSkill> = HashMap::new();

        for skill in existing {
            let dir = skill.repository_dir.clone().unwrap_or_default();

            if new_dirs.contains(&dir) {
                kept.insert(dir, skill);
            } else {
                self.store().delete(&repository.org_id, &skill.id).await?;
                self.remove_objects(&skill.object_prefix).await;
                report.removed += 1;
            }
        }

        for dir in &dirs {
            let relative = relative_to_top(&top, dir).to_string();
            let shown = if relative.is_empty() {
                "/".to_string()
            } else {
                relative.clone()
            };

            let bundle = match SkillBundle::from_files(files_under(&unpacked.files, dir))
                .and_then(|bundle| check_name(&bundle).map(|()| bundle))
            {
                Ok(bundle) => bundle,
                Err(error) => {
                    problems.push(format!("{shown}: {error}"));
                    continue;
                }
            };

            let previous = kept.get(&relative);

            if let Some(other) = self
                .store()
                .find_by_name(&repository.org_id, &repository.scope, &bundle.manifest.name)
                .await?
                && previous.is_none_or(|previous| previous.id != other.id)
            {
                problems.push(format!(
                    "{shown}: a skill named `{}` already exists in this scope",
                    bundle.manifest.name
                ));
                continue;
            }

            if let Some(previous) = previous
                && previous.content_hash == bundle.hash
                && previous.scope == repository.scope
            {
                report.unchanged += 1;
                continue;
            }

            let id = previous.map_or_else(Id::new, |previous| previous.id.clone());
            let prefix = self.store_files(&repository.org_id, &id, &bundle).await?;
            let now = TimestampMicros::now();

            self.store()
                .save(StoredSkill {
                    id: id.clone(),
                    org_id: repository.org_id.clone(),
                    source: SkillSource::Repository,
                    repository_id: Some(repository.id.clone()),
                    repository_dir: Some(relative.clone()),
                    scope: repository.scope.clone(),
                    name: bundle.manifest.name.clone(),
                    description: bundle.manifest.description.clone(),
                    version: bundle.manifest.version.clone(),
                    manifest: bundle.manifest.clone(),
                    files: bundle.index(),
                    content_hash: bundle.hash.clone(),
                    object_prefix: prefix.clone(),
                    package_object_key: None,
                    package_name: None,
                    enabled: previous.is_none_or(|previous| previous.enabled),
                    created_by: previous.map_or_else(
                        || repository.updated_by.clone(),
                        |previous| previous.created_by.clone(),
                    ),
                    updated_by: repository.updated_by.clone(),
                    created_at: previous.map_or(now, |previous| previous.created_at),
                    updated_at: now,
                })
                .await?;

            match previous {
                Some(previous) => {
                    if previous.object_prefix != prefix {
                        self.remove_objects(&previous.object_prefix).await;
                    }

                    report.updated += 1;
                }
                None => report.imported += 1,
            }
        }

        let landed = report.imported + report.updated + report.unchanged;

        report.status = match (problems.is_empty(), landed) {
            (true, _) => RepositoryStatus::Ok,
            (false, 0) => RepositoryStatus::Failed,
            (false, _) => RepositoryStatus::Partial,
        };

        report.problems = problems;
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_in_the_archive_are_read_below_its_top_folder() {
        assert_eq!(under_top("repo-abc", "skills"), "repo-abc/skills");
        assert_eq!(under_top("repo-abc", ""), "repo-abc");
        assert_eq!(under_top("", "skills"), "skills");
        assert_eq!(relative_to_top("repo-abc", "repo-abc/skills/a"), "skills/a");
        assert_eq!(relative_to_top("repo-abc", "repo-abc"), "");
        assert_eq!(relative_to_top("", "skills/a"), "skills/a");
    }

    #[test]
    fn a_failed_sync_says_why() {
        let report = SyncReport::failed(&Error::not_found("no such branch"));

        assert_eq!(report.status, RepositoryStatus::Failed);
        assert_eq!(report.problems.len(), 1);
    }
}
