// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 从 PagerDuty、Opsgenie 迁移排班。
//!
//! 输入是用户自己从对方 REST API 取到的 JSON：服务端不保存对方的 API Key，也不向对方发请求。这里把
//! 响应换成 `molesignal.schedules/v1` 的一项，之后和自家的包走同一条预演和写入流程。
//!
//! 转换守一条规则：同一时刻谁值班不变。轮值表达不了的内容不丢也不猜：会改变值班结果的挡住写入，不影响
//! 现在往后值班结果的明确跳过，都逐条列在预演里（[`Finding`]）。文件里带了对方自己排出来的值班表时，
//! 预演当场拿转换结果和它比（[`rendered`]）。

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde_json::{Map, Value, json};

use super::{Directory, Finding, TransferItem};
use crate::{
    app::alerting::rule_transfer::ImportAction,
    domain::alerting::{
        schedule::{Rotation, RotationKind},
        time_window::parse_timezone,
    },
    shared::{ids::Id, time::TimestampMicros},
};

mod opsgenie;
mod overlap;
mod pagerduty;
mod rendered;
mod weekly;

pub const PAGERDUTY_FORMAT: &str = "pagerduty";
pub const OPSGENIE_FORMAT: &str = "opsgenie";

/// 一份排班最多几条还在生效的轮值：PagerDuty 一份排班最多 60 层。
const MAX_ROTATIONS: usize = 60;
/// 一条轮值最多几段时段限制，再多的文件不是正常取出来的。
const MAX_RESTRICTIONS: usize = 64;

/// 读出来的一个文件。
pub struct ForeignImport {
    pub format: &'static str,
    pub schedules: Vec<ForeignSchedule>,
    /// 带不过来、也不影响别的排班的整份排班，明确跳过。
    pub left_out: Vec<TransferItem>,
    /// 说整个文件的。
    pub findings: Vec<Finding>,
}

/// 转换后的一份排班。
pub struct ForeignSchedule {
    /// `molesignal.schedules/v1` 的一项，成员还是邮箱。
    pub value: Value,
    /// 组织里没有的成员邮箱，包括因为别的原因带不过来的轮换里的。
    pub unknown_members: Vec<String>,
    /// 带不过来、会改变值班结果的内容，有一条就不能写。
    pub blockers: Vec<Finding>,
    /// 明确跳过的内容，以及和对方的值班表比过什么。
    pub notes: Vec<Finding>,
}

/// 读对方的响应。认不出是哪一家（或者是自家的包）时为空。
pub fn read(bundle: &Value, directory: &Directory, now: TimestampMicros) -> Option<ForeignImport> {
    if bundle.get("format").is_some() {
        return None;
    }

    if pagerduty::recognizes(bundle) {
        return Some(pagerduty::read(bundle, directory, now.0));
    }

    opsgenie::recognizes(bundle).then(|| opsgenie::read(bundle, directory, now.0))
}

impl ForeignImport {
    /// 交给预演的那些项，顺序和 [`Self::schedules`] 一样。
    pub fn incoming(&self) -> Vec<Value> {
        self.schedules
            .iter()
            .map(|schedule| schedule.value.clone())
            .collect()
    }

    /// 预演之后把说明写到每一项上：`items` 的前几项和 [`Self::schedules`] 一一对应。带不过来的内容
    /// 让那一项出错；因为「只新建」跳过的项不写，也就不用说。返回说整个文件的那些。
    pub fn annotate(self, items: &mut Vec<TransferItem>) -> Vec<Finding> {
        for (item, schedule) in items.iter_mut().zip(self.schedules) {
            if item.action == ImportAction::Skip {
                continue;
            }

            if !schedule.blockers.is_empty() {
                item.action = ImportAction::Error;
                item.error = Some("this schedule cannot be brought over as it is".into());
                item.blockers = schedule.blockers;
            }

            item.unknown_members = schedule.unknown_members;
            item.findings = schedule.notes;
        }

        items.extend(self.left_out);
        self.findings
    }
}

/// 转换中的一份排班。
struct Draft<'a> {
    directory: &'a Directory,
    name: String,
    description: Option<String>,
    timezone: String,
    enabled: Option<bool>,
    team: Option<String>,
    rotations: Vec<Rotation>,
    unknown_members: Vec<String>,
    blockers: Vec<Finding>,
    notes: Vec<Finding>,
}

impl<'a> Draft<'a> {
    fn new(directory: &'a Directory, name: Option<&str>, timezone: Option<&str>) -> Self {
        Self {
            directory,
            name: name.unwrap_or_default().trim().to_string(),
            description: None,
            timezone: timezone.unwrap_or_default().trim().to_string(),
            enabled: None,
            team: None,
            rotations: Vec::new(),
            unknown_members: Vec::new(),
            blockers: Vec::new(),
            notes: Vec::new(),
        }
    }

    /// 轮值的一个成员，这时还用邮箱当 id。组织里没有的记下来，一个不落地列在预演里。
    fn member(&mut self, email: &str) -> Id {
        let listed = self
            .unknown_members
            .iter()
            .any(|known| known.eq_ignore_ascii_case(email));

        if !listed && !self.directory.users.contains(email) {
            self.unknown_members.push(email.to_string());
        }

        Id::from_string(email)
    }

    fn block(&mut self, finding: Finding) {
        self.blockers.push(finding);
    }

    /// 轮值已经到上限时挡住写入，不再往下转换。
    fn rotations_full(&mut self) -> bool {
        let full = self.rotations.len() >= MAX_ROTATIONS;

        if full {
            self.block(Finding::new("too_many_rotations").with("limit", MAX_ROTATIONS.to_string()));
        }

        full
    }

    fn note(&mut self, finding: Finding) {
        self.notes.push(finding);
    }

    /// 排班的时区；认不出就挡住写入。
    fn zone(&mut self) -> Option<Tz> {
        let zone = parse_timezone(&self.timezone).ok();

        if zone.is_none() {
            let finding = Finding::new("timezone_unknown").with("timezone", self.timezone.clone());

            self.block(finding);
        }

        zone
    }

    /// 对方的团队里第一个在这里有同名团队的作为排班的团队。团队不影响谁值班，其余的只说明一下。
    fn adopt_team<'n>(&mut self, names: impl Iterator<Item = &'n str>) {
        for name in names {
            if self.team.is_none() && self.directory.teams.contains(name) {
                self.team = Some(name.to_string());
            } else {
                self.note(Finding::new("team_not_carried").with("name", name));
            }
        }
    }

    fn finish(self) -> ForeignSchedule {
        let mut value = Map::new();

        value.insert("name".into(), json!(self.name));
        value.insert("timezone".into(), json!(self.timezone));
        value.insert("rotations".into(), json!(self.rotations));

        // 对方没说的设置不写：更新已有的排班时留着它自己的。
        if let Some(description) = self.description {
            value.insert("description".into(), json!(description));
        }

        if let Some(enabled) = self.enabled {
            value.insert("enabled".into(), json!(enabled));
        }

        if let Some(team) = self.team {
            value.insert("team".into(), json!(team));
        }

        ForeignSchedule {
            value: Value::Object(value),
            unknown_members: self.unknown_members,
            blockers: self.blockers,
            notes: self.notes,
        }
    }
}

/// 读带偏移的时间，得到微秒。
fn parse_time(text: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(text.trim())
        .ok()
        .map(|at| at.with_timezone(&Utc).timestamp_micros())
}

/// 写进说明里的时刻：排班时区的本地时间。
fn local_text(at: i64, tz: Tz) -> String {
    format!(
        "{} {}",
        TimestampMicros(at)
            .to_datetime()
            .with_timezone(&tz)
            .format("%Y-%m-%d %H:%M"),
        tz.name()
    )
}

/// 周期（秒）对应的轮换类型：正好一天、一周的用自家的类型，好在界面上认得出来。
fn kind_of_period(seconds: u64) -> RotationKind {
    match seconds {
        86_400 => RotationKind::Daily,
        604_800 => RotationKind::Weekly,
        period_secs => RotationKind::Custom { period_secs },
    }
}

#[cfg(test)]
mod tests;
