// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent Skill：内置、实例目录、上传、仓库四种来源的列出、检视、启停、移交与删除，以及
//! 一个会话里能用哪些 Skill。上传的文件与仓库拉下来的文件存在对象存储里，读出的内容按 hash
//! 缓存。

mod fetch;
mod install;
pub mod instance;
mod repositories;
pub mod session;

use std::{collections::HashMap, sync::Arc};

use moka::sync::Cache;
use object_store::{ObjectStore, ObjectStoreExt as _, path::Path as ObjPath};
use serde::Serialize;

use self::session::{Candidate, Rank};
pub use self::{
    install::{InstallRequest, Installed},
    instance::{InstanceSkills, PathProblem},
    repositories::RepositoryInput,
    session::{Lookup, SessionSkill, SessionSkills, SkillLocation, team_handle},
};
use crate::{
    agent::{
        network::NetworkPolicyStore,
        scope::{ChatScope, Scope, Viewer},
        skills::{
            FileEntry, SkillBundle, SkillFile, SkillManifest, SkillSource, SkillStore, StoredSkill,
            is_text,
        },
    },
    domain::{i18n_language::LanguagePack, iam::TeamRepository},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 对象存储里 Skill 文件的根。
const OBJECT_ROOT: &str = "agent_skills/v1";

/// 设置页里的一个 Skill。
#[derive(Debug, Clone, Serialize)]
pub struct SkillView {
    /// 实例级 Skill 是 `builtin:<名称>` / `path:<名称>`，其余是数据库 ID。
    pub id: String,
    pub name: String,
    /// 给人看的名字：内置 Skill 按读者语言，其余就是名称。
    pub title: String,
    pub description: String,
    pub version: Option<String>,
    pub source: SkillSource,
    /// `instance`、`org`、`team`、`personal`。
    pub scope: &'static str,
    pub team_id: Option<String>,
    pub owner_id: Option<String>,
    pub enabled: bool,
    /// 能替换、移交（改内容与作用域）。
    pub editable: bool,
    /// 能启停、删除。
    pub moderatable: bool,
    pub hash: String,
    pub file_count: usize,
    pub argument_hint: Option<String>,
    pub allowed_tools: Vec<String>,
    pub tags: Vec<String>,
    pub examples: Vec<String>,
    pub requires_runner: bool,
    pub repository_id: Option<String>,
    pub package_name: Option<String>,
    /// 实例目录来源的服务器目录。
    pub dir: Option<String>,
    pub updated_at_micros: Option<i64>,
}

/// 检视抽屉的内容。
#[derive(Debug, Clone, Serialize)]
pub struct SkillDetail {
    #[serde(flatten)]
    pub view: SkillView,
    /// `SKILL.md` 去掉 frontmatter 的正文。
    pub body: String,
    pub files: Vec<FileEntry>,
}

pub struct SkillService {
    store: Arc<dyn SkillStore>,
    objects: Arc<dyn ObjectStore>,
    instance: Arc<InstanceSkills>,
    network: Arc<dyn NetworkPolicyStore>,
    teams: Arc<dyn TeamRepository>,
    bundles: Cache<String, Arc<SkillBundle>>,
}

fn stored_view(skill: &StoredSkill, viewer: &Viewer) -> SkillView {
    SkillView {
        id: skill.id.0.clone(),
        name: skill.name.clone(),
        title: skill.name.clone(),
        description: skill.description.clone(),
        version: skill.version.clone(),
        source: skill.source,
        scope: skill.scope.kind(),
        team_id: skill.scope.team_id().map(|id| id.0.clone()),
        owner_id: skill.scope.owner_id().map(|id| id.0.clone()),
        enabled: skill.enabled,
        editable: viewer.can_edit(&skill.scope),
        moderatable: viewer.can_moderate(&skill.scope),
        hash: skill.content_hash.clone(),
        file_count: skill.files.len(),
        argument_hint: skill.manifest.argument_hint.clone(),
        allowed_tools: skill.manifest.allowed_tools.clone(),
        tags: skill.manifest.tags.clone(),
        examples: skill.manifest.examples.clone(),
        requires_runner: skill.manifest.requires_runner,
        repository_id: skill.repository_id.as_ref().map(|id| id.0.clone()),
        package_name: skill.package_name.clone(),
        dir: skill.repository_dir.clone(),
        updated_at_micros: Some(skill.updated_at.0),
    }
}

fn instance_view(skill: &instance::InstanceSkill, enabled: bool, viewer: &Viewer) -> SkillView {
    let manifest = &skill.bundle.manifest;

    SkillView {
        id: skill.key.clone(),
        name: manifest.name.clone(),
        title: manifest.name.clone(),
        description: manifest.description.clone(),
        version: manifest.version.clone(),
        source: skill.source,
        scope: "instance",
        team_id: None,
        owner_id: None,
        enabled,
        editable: false,
        moderatable: viewer.manager,
        hash: skill.bundle.hash.clone(),
        file_count: skill.bundle.files.len(),
        argument_hint: manifest.argument_hint.clone(),
        allowed_tools: manifest.allowed_tools.clone(),
        tags: manifest.tags.clone(),
        examples: manifest.examples.clone(),
        requires_runner: manifest.requires_runner,
        repository_id: None,
        package_name: None,
        dir: skill.dir.clone(),
        updated_at_micros: None,
    }
}

/// 内置 Skill 读者语言文字的键前缀：`agent.skill.bundled.<名称>.<部分>`。
pub const BUNDLED_SKILL_KEY: &str = "agent.skill.bundled";

/// 内置 Skill 的一段读者语言文字：`title`、`description` 或 `argument_hint`。语言键只用小写字母、
/// 数字、下划线和点，名称里的连字符写成下划线。
pub fn bundled_text<'a>(texts: &'a LanguagePack, name: &str, part: &str) -> Option<&'a str> {
    texts.get(&format!(
        "{BUNDLED_SKILL_KEY}.{}.{part}",
        name.replace('-', "_")
    ))
}

impl SkillView {
    /// 内置 Skill 换成读者语言的标题、说明与参数提示（见 [`bundled_text`]）。
    pub fn localized(mut self, texts: &LanguagePack) -> Self {
        if self.source == SkillSource::Builtin {
            if let Some(title) = bundled_text(texts, &self.name, "title") {
                self.title = title.to_string();
            }

            if let Some(description) = bundled_text(texts, &self.name, "description") {
                self.description = description.to_string();
            }

            if let Some(hint) = bundled_text(texts, &self.name, "argument_hint") {
                self.argument_hint = Some(hint.to_string());
            }
        }

        self
    }
}

/// 对象存储里一个版本的文件前缀。
pub(super) fn object_prefix(org_id: &Id, skill_id: &Id, hash: &str) -> String {
    format!("{OBJECT_ROOT}/{}/{}/{hash}/", org_id.0, skill_id.0)
}

impl SkillService {
    pub fn new(
        store: Arc<dyn SkillStore>,
        objects: Arc<dyn ObjectStore>,
        instance: Arc<InstanceSkills>,
        network: Arc<dyn NetworkPolicyStore>,
        teams: Arc<dyn TeamRepository>,
    ) -> Self {
        Self {
            store,
            objects,
            instance,
            network,
            teams,
            bundles: Cache::new(256),
        }
    }

    pub fn instance(&self) -> &Arc<InstanceSkills> {
        &self.instance
    }

    /// 设置页的列表：实例级 Skill 都列，存的 Skill 按作用域只列看得到的。
    pub async fn list(&self, org_id: &Id, viewer: &Viewer) -> Result<Vec<SkillView>> {
        let settings = self.store.instance_settings(org_id).await?;

        let mut views: Vec<SkillView> = self
            .instance
            .all()
            .iter()
            .map(|skill| {
                let enabled = settings.get(&skill.key).copied().unwrap_or(true);
                instance_view(skill, enabled, viewer)
            })
            .collect();

        views.extend(
            self.store
                .list(org_id)
                .await?
                .iter()
                .filter(|skill| viewer.can_see(&skill.scope))
                .map(|skill| stored_view(skill, viewer)),
        );

        Ok(views)
    }

    /// 看得到的一个 Skill；看不到与不存在一样。
    async fn visible(&self, org_id: &Id, viewer: &Viewer, id: &str) -> Result<SkillLocation> {
        if let Some(skill) = self.instance.find(id) {
            return Ok(SkillLocation::Instance(skill));
        }

        let skill = self.store.get(org_id, &Id::from_string(id)).await?;

        if !viewer.can_see(&skill.scope) {
            return Err(Error::not_found(format!("skill `{id}` not found")));
        }

        Ok(SkillLocation::Stored(Box::new(skill)))
    }

    pub async fn detail(&self, org_id: &Id, viewer: &Viewer, id: &str) -> Result<SkillDetail> {
        let location = self.visible(org_id, viewer, id).await?;
        let bundle = self.bundle(&location).await?;

        let view = match &location {
            SkillLocation::Instance(skill) => {
                let enabled = self
                    .store
                    .instance_settings(org_id)
                    .await?
                    .get(&skill.key)
                    .copied()
                    .unwrap_or(true);

                instance_view(skill, enabled, viewer)
            }
            SkillLocation::Stored(skill) => stored_view(skill, viewer),
        };

        Ok(SkillDetail {
            view,
            body: bundle.body.clone(),
            files: bundle.index(),
        })
    }

    /// 一个文本文件的内容。
    pub async fn file(&self, org_id: &Id, viewer: &Viewer, id: &str, path: &str) -> Result<String> {
        let location = self.visible(org_id, viewer, id).await?;
        let bundle = self.bundle(&location).await?;
        text_file(&bundle, path)
    }

    /// Skill 的全部内容。存的 Skill 从对象存储读，按 hash 缓存。
    pub async fn bundle(&self, location: &SkillLocation) -> Result<Arc<SkillBundle>> {
        let skill = match location {
            SkillLocation::Instance(skill) => return Ok(Arc::new(skill.bundle.clone())),
            SkillLocation::Stored(skill) => skill,
        };

        if let Some(bundle) = self.bundles.get(&skill.content_hash) {
            return Ok(bundle);
        }

        let mut files = Vec::with_capacity(skill.files.len());

        for entry in &skill.files {
            let path = ObjPath::from(format!("{}{}", skill.object_prefix, entry.path));

            let bytes = self
                .objects
                .get(&path)
                .await
                .map_err(|error| {
                    Error::unavailable(format!("skill file `{}`: {error}", entry.path))
                })?
                .bytes()
                .await
                .map_err(|error| {
                    Error::unavailable(format!("skill file `{}`: {error}", entry.path))
                })?;

            files.push(SkillFile {
                path: entry.path.clone(),
                bytes: bytes.to_vec(),
            });
        }

        let bundle = Arc::new(SkillBundle::from_files(files)?);
        self.bundles
            .insert(skill.content_hash.clone(), bundle.clone());
        Ok(bundle)
    }

    /// 启停：实例级 Skill 记在组织设置里（`agent.manage`），存的 Skill 由能管理它的人改。
    pub async fn set_enabled(
        &self,
        org_id: &Id,
        viewer: &Viewer,
        id: &str,
        enabled: bool,
    ) -> Result<()> {
        match self.visible(org_id, viewer, id).await? {
            SkillLocation::Instance(skill) => {
                if !viewer.manager {
                    return Err(Error::forbidden(
                        "switching a built-in or instance skill needs agent.manage",
                    ));
                }

                self.store
                    .set_instance_enabled(org_id, &skill.key, enabled, &viewer.user_id)
                    .await
            }
            SkillLocation::Stored(mut skill) => {
                if !viewer.can_moderate(&skill.scope) {
                    return Err(Error::forbidden("this skill belongs to someone else"));
                }

                skill.enabled = enabled;
                skill.updated_by = viewer.user_id.clone();
                skill.updated_at = TimestampMicros::now();
                self.store.save(*skill).await.map(|_| ())
            }
        }
    }

    /// 移交到另一个作用域：两边都要有权限，目标作用域里不能重名。
    pub async fn move_to(
        &self,
        org_id: &Id,
        viewer: &Viewer,
        id: &str,
        target: Scope,
    ) -> Result<StoredSkill> {
        let SkillLocation::Stored(mut skill) = self.visible(org_id, viewer, id).await? else {
            return Err(Error::invalid(
                "built-in and instance skills cannot be moved",
            ));
        };

        if skill.source == SkillSource::Repository {
            return Err(Error::invalid(
                "a skill from a repository moves with its repository",
            ));
        }

        if !viewer.can_edit(&skill.scope) || !viewer.can_edit(&target) {
            return Err(Error::forbidden(
                "moving a skill needs permission in both scopes",
            ));
        }

        skill.scope = target;
        skill.updated_by = viewer.user_id.clone();
        skill.updated_at = TimestampMicros::now();
        self.store.save(*skill).await
    }

    pub async fn delete(&self, org_id: &Id, viewer: &Viewer, id: &str) -> Result<StoredSkill> {
        let SkillLocation::Stored(skill) = self.visible(org_id, viewer, id).await? else {
            return Err(Error::invalid(
                "built-in and instance skills can only be switched off",
            ));
        };

        if skill.source == SkillSource::Repository {
            return Err(Error::invalid(
                "remove the skill from its repository, or delete the repository",
            ));
        }

        if !viewer.can_moderate(&skill.scope) {
            return Err(Error::forbidden("this skill belongs to someone else"));
        }

        self.store.delete(org_id, &skill.id).await?;
        self.remove_objects(&format!("{OBJECT_ROOT}/{}/{}/", org_id.0, skill.id.0))
            .await;

        Ok(*skill)
    }

    /// 尽力删掉一个前缀下的对象；失败只记日志。
    pub(super) async fn remove_objects(&self, prefix: &str) {
        use futures::StreamExt as _;

        let prefix = ObjPath::from(prefix);
        let mut objects = self.objects.list(Some(&prefix));

        while let Some(object) = objects.next().await {
            match object {
                Ok(object) => {
                    if let Err(error) = self.objects.delete(&object.location).await {
                        tracing::warn!(%error, "skill object was not deleted");
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, "skill objects could not be listed");
                    return;
                }
            }
        }
    }

    /// 团队 ID → 在 `@` 后的写法。
    async fn team_handles(&self, org_id: &Id) -> Result<HashMap<Id, String>> {
        Ok(self
            .teams
            .list(org_id)
            .await?
            .into_iter()
            .map(|team| (team.id, team_handle(&team.name)))
            .collect())
    }

    /// 一个会话里能用的 Skill（只含启用的）。
    pub async fn session(&self, org_id: &Id, chat: &ChatScope) -> Result<SessionSkills> {
        let settings = self.store.instance_settings(org_id).await?;
        let handles = self.team_handles(org_id).await?;
        let mut candidates = Vec::new();

        for skill in self.instance.all() {
            if !settings.get(&skill.key).copied().unwrap_or(true) {
                continue;
            }

            let rank = if skill.source == SkillSource::Builtin {
                Rank::Builtin
            } else {
                Rank::Path
            };

            candidates.push(Candidate {
                rank,
                skill: session_skill_from_instance(skill),
            });
        }

        for skill in self.store.list(org_id).await? {
            if !skill.enabled || !chat.loads(&skill.scope) {
                continue;
            }

            let rank = match &skill.scope {
                Scope::Org => Rank::Org,
                Scope::Team { .. } => Rank::Team,
                Scope::Personal { .. } => Rank::Personal,
            };

            candidates.push(Candidate {
                rank,
                skill: session_skill_from_stored(skill, &handles),
            });
        }

        Ok(SessionSkills::resolve(candidates))
    }

    /// 个人会话里还没挂进来、本人所在团队的 Skill：写成 `名称@团队`，调用时挂进会话。
    pub async fn mountable(
        &self,
        org_id: &Id,
        viewer: &Viewer,
        chat: &ChatScope,
    ) -> Result<Vec<SessionSkill>> {
        let ChatScope::Personal { mounted_teams, .. } = chat else {
            return Ok(Vec::new());
        };

        let handles = self.team_handles(org_id).await?;

        Ok(self
            .store
            .list(org_id)
            .await?
            .into_iter()
            .filter(|skill| skill.enabled)
            .filter_map(|skill| {
                let team = skill.scope.team_id()?.clone();

                (viewer.is_member(&team) && !mounted_teams.contains(&team))
                    .then(|| session_skill_from_stored(skill, &handles))
            })
            .map(|mut skill| {
                if let Some((_, handle)) = &skill.team {
                    skill.invocation = format!("{}@{handle}", skill.name);
                }

                skill
            })
            .collect())
    }

    pub(super) fn network(&self) -> &Arc<dyn NetworkPolicyStore> {
        &self.network
    }

    pub(super) fn store(&self) -> &Arc<dyn SkillStore> {
        &self.store
    }

    /// 存储本身，给要直接查 Skill 与仓库的地方（删除团队前的检查）。
    pub fn store_handle(&self) -> &Arc<dyn SkillStore> {
        &self.store
    }

    pub(super) fn objects(&self) -> &Arc<dyn ObjectStore> {
        &self.objects
    }
}

/// 一个文本文件；二进制或太大的不给。
pub fn text_file(bundle: &SkillBundle, path: &str) -> Result<String> {
    let file = bundle
        .file(path.trim_start_matches("./"))
        .ok_or_else(|| Error::not_found(format!("the skill has no file `{path}`")))?;

    if !is_text(&file.bytes) {
        return Err(Error::invalid(format!(
            "`{path}` is not a text file the agent can read"
        )));
    }

    Ok(String::from_utf8_lossy(&file.bytes).into_owned())
}

fn session_skill_from_instance(skill: Arc<instance::InstanceSkill>) -> SessionSkill {
    let manifest: SkillManifest = skill.bundle.manifest.clone();

    SessionSkill {
        id: skill.key.clone(),
        name: manifest.name.clone(),
        invocation: manifest.name.clone(),
        description: manifest.description.clone(),
        version: manifest.version.clone(),
        hash: skill.bundle.hash.clone(),
        source: skill.source,
        team: None,
        manifest,
        location: SkillLocation::Instance(skill),
    }
}

fn session_skill_from_stored(skill: StoredSkill, handles: &HashMap<Id, String>) -> SessionSkill {
    let team = skill.scope.team_id().map(|id| {
        let handle = handles.get(id).cloned().unwrap_or_else(|| id.0.clone());
        (id.clone(), handle)
    });

    SessionSkill {
        id: skill.id.0.clone(),
        name: skill.name.clone(),
        invocation: skill.name.clone(),
        description: skill.description.clone(),
        version: skill.version.clone(),
        hash: skill.content_hash.clone(),
        source: skill.source,
        team,
        manifest: skill.manifest.clone(),
        location: SkillLocation::Stored(Box::new(skill)),
    }
}
