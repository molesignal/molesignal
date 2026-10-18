// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 上传一个 Skill 包：解开、校验、存进对象存储，再记一行。同一作用域里已经有同名 Skill 时，
//! 要明说替换：替换保留原 ID，`/名称` 照旧可用，旧版本的文件随后删除。

use object_store::{ObjectStoreExt as _, PutPayload, path::Path as ObjPath};

use super::{OBJECT_ROOT, SkillService, object_prefix};
use crate::{
    agent::{
        commands::find_builtin,
        scope::{Scope, Viewer},
        skills::{
            MAX_PACKAGE_BYTES, PACKAGE_LIMITS, SkillBundle, SkillSource, StoredSkill, archive_kind,
            bundle_from_package, unpack, zip_files,
        },
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 一次上传。
pub struct InstallRequest {
    pub scope: Scope,
    pub file_name: String,
    pub bytes: Vec<u8>,
    /// 同名时替换；不替换时同名报冲突。
    pub replace: bool,
}

/// 上传的结果：存好了，或同一作用域里已有同名 Skill、等用户确认替换。
#[derive(Debug)]
pub enum Installed {
    Saved(Box<StoredSkill>),
    Exists { name: String },
}

/// 名称不能与内置命令重名：`/名称` 要能调用它。
pub(super) fn check_name(bundle: &SkillBundle) -> Result<()> {
    if find_builtin(&bundle.manifest.name.to_ascii_lowercase()).is_some() {
        return Err(Error::invalid(format!(
            "`{}` is a built-in command; rename the skill",
            bundle.manifest.name
        )));
    }

    Ok(())
}

impl SkillService {
    /// 把一个版本的文件写进对象存储，返回前缀。
    pub(super) async fn store_files(
        &self,
        org_id: &Id,
        skill_id: &Id,
        bundle: &SkillBundle,
    ) -> Result<String> {
        let prefix = object_prefix(org_id, skill_id, &bundle.hash);

        for file in &bundle.files {
            self.objects()
                .put(
                    &ObjPath::from(format!("{prefix}{}", file.path)),
                    PutPayload::from(file.bytes.clone()),
                )
                .await
                .map_err(|error| Error::unavailable(format!("skill file write: {error}")))?;
        }

        Ok(prefix)
    }

    pub async fn install(
        &self,
        org_id: &Id,
        viewer: &Viewer,
        request: InstallRequest,
    ) -> Result<Installed> {
        if !viewer.can_edit(&request.scope) {
            return Err(Error::forbidden("you cannot add skills to this scope"));
        }

        if request.bytes.len() > MAX_PACKAGE_BYTES {
            return Err(Error::payload_too_large(format!(
                "a skill package is at most {} MiB",
                MAX_PACKAGE_BYTES / 1024 / 1024
            )));
        }

        let kind = archive_kind(&request.file_name)
            .ok_or_else(|| Error::invalid("upload a .zip, .skill, .tar.gz or .tgz package"))?;

        let unpacked = unpack(&request.bytes, kind, PACKAGE_LIMITS, |_| true)?;
        let bundle = bundle_from_package(unpacked.files)?;
        check_name(&bundle)?;

        let existing = self
            .store()
            .find_by_name(org_id, &request.scope, &bundle.manifest.name)
            .await?;

        if let Some(existing) = &existing {
            if !request.replace {
                return Ok(Installed::Exists {
                    name: existing.name.clone(),
                });
            }

            if existing.source != SkillSource::Upload {
                return Err(Error::conflict(format!(
                    "`{}` in this scope comes from a repository; change it there",
                    existing.name
                )));
            }
        }

        let now = TimestampMicros::now();
        let id = existing
            .as_ref()
            .map_or_else(Id::new, |existing| existing.id.clone());

        let prefix = self.store_files(org_id, &id, &bundle).await?;

        let extension = request
            .file_name
            .to_ascii_lowercase()
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_string())
            .unwrap_or_else(|| "zip".into());

        let package_key = format!(
            "{OBJECT_ROOT}/{}/{}/packages/{}.{extension}",
            org_id.0, id.0, bundle.hash
        );

        self.objects()
            .put(
                &ObjPath::from(package_key.as_str()),
                PutPayload::from(request.bytes.clone()),
            )
            .await
            .map_err(|error| Error::unavailable(format!("skill package write: {error}")))?;

        let skill = StoredSkill {
            id: id.clone(),
            org_id: org_id.clone(),
            source: SkillSource::Upload,
            repository_id: None,
            repository_dir: None,
            scope: request.scope,
            name: bundle.manifest.name.clone(),
            description: bundle.manifest.description.clone(),
            version: bundle.manifest.version.clone(),
            manifest: bundle.manifest.clone(),
            files: bundle.index(),
            content_hash: bundle.hash.clone(),
            object_prefix: prefix.clone(),
            package_object_key: Some(package_key.clone()),
            package_name: Some(request.file_name.clone()),
            enabled: existing.as_ref().is_none_or(|existing| existing.enabled),
            created_by: existing.as_ref().map_or_else(
                || viewer.user_id.clone(),
                |existing| existing.created_by.clone(),
            ),
            updated_by: viewer.user_id.clone(),
            created_at: existing
                .as_ref()
                .map_or(now, |existing| existing.created_at),
            updated_at: now,
        };

        let saved = self.store().save(skill).await?;

        // 替换后旧版本的文件与原包不再用到。
        if let Some(previous) = existing
            && previous.content_hash != saved.content_hash
        {
            self.remove_objects(&previous.object_prefix).await;

            if let Some(key) = previous.package_object_key
                && key != package_key
                && let Err(error) = self.objects().delete(&ObjPath::from(key.as_str())).await
            {
                tracing::warn!(%error, "old skill package was not deleted");
            }
        }

        Ok(Installed::Saved(Box::new(saved)))
    }

    /// 下载：上传的给原包，其他来源打成 zip。
    pub async fn package(
        &self,
        org_id: &Id,
        viewer: &Viewer,
        id: &str,
    ) -> Result<(String, Vec<u8>)> {
        let location = self.visible(org_id, viewer, id).await?;

        if let super::SkillLocation::Stored(skill) = &location
            && let (Some(key), Some(name)) = (&skill.package_object_key, &skill.package_name)
        {
            let bytes = self
                .objects()
                .get(&ObjPath::from(key.as_str()))
                .await
                .map_err(|error| Error::unavailable(format!("skill package read: {error}")))?
                .bytes()
                .await
                .map_err(|error| Error::unavailable(format!("skill package read: {error}")))?;

            return Ok((name.clone(), bytes.to_vec()));
        }

        let bundle = self.bundle(&location).await?;
        let zipped = zip_files(&bundle.manifest.name, &bundle.files)?;
        Ok((format!("{}.zip", bundle.manifest.name), zipped))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::skills::{ArchiveKind, SKILL_FILE, SkillFile};

    #[test]
    fn a_skill_cannot_take_the_name_of_a_command() {
        let bundle = |name: &str| {
            SkillBundle::from_files(vec![SkillFile {
                path: SKILL_FILE.into(),
                bytes: format!("---\nname: {name}\ndescription: x\n---\n").into_bytes(),
            }])
            .unwrap()
        };

        assert!(check_name(&bundle("Help")).is_err());
        assert!(check_name(&bundle("runbook")).is_ok());
    }

    #[test]
    fn a_zipped_skill_unpacks_into_the_same_skill() {
        let bundle = SkillBundle::from_files(vec![
            SkillFile {
                path: SKILL_FILE.into(),
                bytes: b"---\nname: runbook\ndescription: x\n---\nBody".to_vec(),
            },
            SkillFile {
                path: "refs/a.md".into(),
                bytes: b"a".to_vec(),
            },
        ])
        .unwrap();

        let zipped = zip_files(&bundle.manifest.name, &bundle.files).unwrap();
        let files = unpack(&zipped, ArchiveKind::Zip, PACKAGE_LIMITS, |_| true)
            .unwrap()
            .files;

        assert_eq!(bundle_from_package(files).unwrap().hash, bundle.hash);
    }
}
