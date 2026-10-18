// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一个会话里能用哪些 Skill、各叫什么。
//!
//! 同名时越具体的越优先：个人 > 团队 > 组织 > 实例目录 > 内置，被盖住的不再出现。个人会话里
//! 挂了几个团队、它们都有同名 Skill 时有歧义：这些 Skill 都写成 `名称@团队`，裸名调用被拒绝
//! 并列出可选写法。团队写法是去掉空白的团队名（不分大小写）或团队 ID。

use std::{collections::BTreeMap, sync::Arc};

use super::instance::InstanceSkill;
use crate::{
    agent::skills::{CatalogEntry, SkillManifest, SkillSource, StoredSkill},
    shared::ids::Id,
};

/// Skill 的内容在哪。
#[derive(Debug, Clone)]
pub enum SkillLocation {
    Instance(Arc<InstanceSkill>),
    Stored(Box<StoredSkill>),
}

/// 会话里的一个 Skill。
#[derive(Debug, Clone)]
pub struct SessionSkill {
    /// 实例级 Skill 的键，或数据库里的 ID。
    pub id: String,
    pub name: String,
    /// 在会话里调用它的写法：名称，有歧义时 `名称@团队`。
    pub invocation: String,
    pub description: String,
    pub version: Option<String>,
    pub hash: String,
    pub source: SkillSource,
    /// 团队 Skill 的团队与它的写法。
    pub team: Option<(Id, String)>,
    pub manifest: SkillManifest,
    pub location: SkillLocation,
}

/// 越小越优先。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rank {
    Personal = 0,
    Team = 1,
    Org = 2,
    Path = 3,
    Builtin = 4,
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub rank: Rank,
    pub skill: SessionSkill,
}

/// 团队在 `@` 后面的写法：去掉空白的名称。
pub fn team_handle(name: &str) -> String {
    name.chars().filter(|c| !c.is_whitespace()).collect()
}

#[derive(Debug)]
pub enum Lookup<'a> {
    Found(&'a SessionSkill),
    /// 裸名有歧义，列出可选写法。
    Ambiguous(Vec<String>),
    Missing,
}

#[derive(Debug, Default, Clone)]
pub struct SessionSkills {
    pub skills: Vec<SessionSkill>,
    /// 小写名称 → 可选写法。
    ambiguous: BTreeMap<String, Vec<String>>,
}

impl SessionSkills {
    pub fn resolve(candidates: Vec<Candidate>) -> Self {
        let mut by_name: BTreeMap<String, Vec<Candidate>> = BTreeMap::new();

        for candidate in candidates {
            by_name
                .entry(candidate.skill.name.to_ascii_lowercase())
                .or_default()
                .push(candidate);
        }

        let mut resolved = Self::default();

        for (name, mut group) in by_name {
            group.sort_by_key(|candidate| candidate.rank);

            let best = group[0].rank;
            let winners: Vec<Candidate> = group
                .into_iter()
                .filter(|candidate| candidate.rank == best)
                .collect();

            if winners.len() == 1 {
                if let Some(winner) = winners.into_iter().next() {
                    let mut skill = winner.skill;
                    skill.invocation = skill.name.clone();
                    resolved.skills.push(skill);
                }

                continue;
            }

            // 只有团队之间会同名并列：几个挂进来的团队各有一个。
            let mut options = Vec::new();

            for winner in winners {
                let mut skill = winner.skill;

                skill.invocation = match &skill.team {
                    Some((_, handle)) => format!("{}@{handle}", skill.name),
                    None => skill.name.clone(),
                };

                options.push(skill.invocation.clone());
                resolved.skills.push(skill);
            }

            resolved.ambiguous.insert(name, options);
        }

        resolved.skills.sort_by(|left, right| {
            left.invocation
                .to_ascii_lowercase()
                .cmp(&right.invocation.to_ascii_lowercase())
        });

        resolved
    }

    /// 按 `名称` 或 `名称@团队` 找。
    pub fn find(&self, name: &str, team: Option<&str>) -> Lookup<'_> {
        let lower = name.to_ascii_lowercase();

        if let Some(team) = team {
            let team = team.to_ascii_lowercase();

            return self
                .skills
                .iter()
                .find(|skill| {
                    skill.name.to_ascii_lowercase() == lower
                        && skill.team.as_ref().is_some_and(|(id, handle)| {
                            handle.to_ascii_lowercase() == team || id.0.to_ascii_lowercase() == team
                        })
                })
                .map_or(Lookup::Missing, Lookup::Found);
        }

        if let Some(options) = self.ambiguous.get(&lower) {
            return Lookup::Ambiguous(options.clone());
        }

        self.skills
            .iter()
            .find(|skill| skill.invocation.to_ascii_lowercase() == lower)
            .map_or(Lookup::Missing, Lookup::Found)
    }

    pub fn catalog(&self) -> Vec<CatalogEntry> {
        self.skills
            .iter()
            .map(|skill| CatalogEntry {
                name: skill.invocation.clone(),
                description: skill.description.clone(),
            })
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::skills::{SKILL_FILE, SkillBundle, SkillFile};

    fn skill(name: &str, source: SkillSource, team: Option<&str>) -> SessionSkill {
        let bundle = SkillBundle::from_files(vec![SkillFile {
            path: SKILL_FILE.into(),
            bytes: format!("---\nname: {name}\ndescription: {name} skill\n---\n").into_bytes(),
        }])
        .unwrap();

        SessionSkill {
            id: format!("{name}-{source:?}-{}", team.unwrap_or("")),
            name: name.into(),
            invocation: String::new(),
            description: bundle.manifest.description.clone(),
            version: None,
            hash: bundle.hash.clone(),
            source,
            team: team.map(|team| (Id::from_string(format!("id-{team}")), team.to_string())),
            manifest: bundle.manifest.clone(),
            location: SkillLocation::Instance(Arc::new(InstanceSkill {
                key: format!("builtin:{name}"),
                source,
                dir: None,
                bundle,
            })),
        }
    }

    fn candidate(rank: Rank, skill: SessionSkill) -> Candidate {
        Candidate { rank, skill }
    }

    #[test]
    fn the_most_specific_skill_wins_its_name() {
        let skills = SessionSkills::resolve(vec![
            candidate(Rank::Builtin, skill("triage", SkillSource::Builtin, None)),
            candidate(Rank::Org, skill("triage", SkillSource::Upload, None)),
            candidate(Rank::Personal, skill("triage", SkillSource::Upload, None)),
            candidate(Rank::Builtin, skill("latency", SkillSource::Builtin, None)),
        ]);

        assert_eq!(skills.skills.len(), 2);

        match skills.find("Triage", None) {
            Lookup::Found(found) => assert!(found.id.starts_with("triage-Upload")),
            other => panic!("{other:?}"),
        }

        assert_eq!(skills.catalog()[0].name, "latency");
    }

    #[test]
    fn two_mounted_teams_with_one_name_need_the_team() {
        let skills = SessionSkills::resolve(vec![
            candidate(
                Rank::Team,
                skill("runbook", SkillSource::Upload, Some("SRE")),
            ),
            candidate(
                Rank::Team,
                skill("runbook", SkillSource::Upload, Some("DBA")),
            ),
            candidate(Rank::Org, skill("runbook", SkillSource::Upload, None)),
        ]);

        match skills.find("runbook", None) {
            Lookup::Ambiguous(options) => {
                assert_eq!(options.len(), 2);
                assert!(options.contains(&"runbook@SRE".to_string()));
            }
            other => panic!("{other:?}"),
        }

        assert!(matches!(
            skills.find("runbook", Some("sre")),
            Lookup::Found(_)
        ));
        assert!(matches!(
            skills.find("runbook", Some("id-dba")),
            Lookup::Found(_)
        ));
        assert!(matches!(
            skills.find("runbook", Some("ops")),
            Lookup::Missing
        ));
        assert!(matches!(skills.find("nothing", None), Lookup::Missing));
    }

    #[test]
    fn a_team_is_written_without_spaces() {
        assert_eq!(team_handle("SRE 值班组"), "SRE值班组");
    }
}
