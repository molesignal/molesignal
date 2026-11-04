// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 导出时把 id 换成名字、导入时换回来：用户按邮箱，团队、排班、升级策略、节假日日历按名称。
//!
//! 换的是 JSON 里这几个键（哪一层都算）：`user_id` ↔ `user`、`team_id` ↔ `team`、`schedule_id` ↔
//! `schedule`、`policy_id` ↔ `policy`、`calendar_id` ↔ `calendar`，以及轮值的成员列表 `members`。

use std::collections::HashMap;

use serde_json::{Map, Value};

use crate::shared::{Error, Result, ids::Id};

/// 一类东西的 id 与名字。
#[derive(Debug, Default, Clone)]
pub struct Names {
    by_id: HashMap<Id, String>,
    /// 名字（用户的邮箱不分大小写）对应的 id；同名的有几个时记下全部。
    by_name: HashMap<String, Vec<Id>>,
    fold_case: bool,
}

impl Names {
    pub fn new(entries: impl IntoIterator<Item = (Id, String)>, fold_case: bool) -> Self {
        let mut names = Self {
            fold_case,
            ..Self::default()
        };

        for (id, name) in entries {
            names
                .by_name
                .entry(names.key(&name))
                .or_default()
                .push(id.clone());
            names.by_id.insert(id, name);
        }

        names
    }

    fn key(&self, name: &str) -> String {
        if self.fold_case {
            name.trim().to_lowercase()
        } else {
            name.trim().to_string()
        }
    }

    fn name_of(&self, id: &str) -> Option<&String> {
        self.by_id.get(&Id(id.to_string()))
    }

    /// 有叫这个名字的，同名的有几个也算。
    pub fn contains(&self, name: &str) -> bool {
        self.by_name.contains_key(&self.key(name))
    }

    fn id_of(&self, what: &str, name: &str) -> Result<Id> {
        match self.by_name.get(&self.key(name)).map(Vec::as_slice) {
            Some([id]) => Ok(id.clone()),
            Some([]) | None => Err(Error::invalid(format!("no {what} is named `{name}`"))),
            Some(_) => Err(Error::invalid(format!(
                "several {what}s are named `{name}`; give them different names first"
            ))),
        }
    }
}

/// 一个组织里能被引用的东西。
#[derive(Debug, Default, Clone)]
pub struct Directory {
    pub users: Names,
    pub teams: Names,
    pub schedules: Names,
    pub policies: Names,
    pub calendars: Names,
}

/// 换的键：JSON 里的 id 键、换成的名字键、哪一类、错误里怎么叫它。
const KEYS: [(&str, &str, Kind, &str); 5] = [
    ("userId", "user", Kind::User, "user"),
    ("teamId", "team", Kind::Team, "team"),
    ("scheduleId", "schedule", Kind::Schedule, "schedule"),
    ("policyId", "policy", Kind::Policy, "escalation policy"),
    ("calendarId", "calendar", Kind::Calendar, "holiday calendar"),
];

#[derive(Debug, Clone, Copy)]
enum Kind {
    User,
    Team,
    Schedule,
    Policy,
    Calendar,
}

impl Directory {
    fn names(&self, kind: Kind) -> &Names {
        match kind {
            Kind::User => &self.users,
            Kind::Team => &self.teams,
            Kind::Schedule => &self.schedules,
            Kind::Policy => &self.policies,
            Kind::Calendar => &self.calendars,
        }
    }

    /// 把 id 换成名字。找不到的 id（引用的东西已经删了）原样留着，导入时会报出来。
    pub fn name_references(&self, value: &mut Value) {
        match value {
            Value::Object(object) => {
                for (id_key, name_key, kind, _) in KEYS {
                    if let Some(Value::String(id)) = object.get(id_key).cloned() {
                        let name = self.names(kind).name_of(&id).cloned().unwrap_or(id);
                        object.remove(id_key);
                        object.insert(name_key.into(), Value::String(name));
                    }
                }

                if let Some(Value::Array(members)) = object.get_mut("members") {
                    for member in members.iter_mut() {
                        if let Value::String(id) = member
                            && let Some(email) = self.users.name_of(id)
                        {
                            *member = Value::String(email.clone());
                        }
                    }
                }

                object
                    .values_mut()
                    .for_each(|child| self.name_references(child));
            }
            Value::Array(items) => items.iter_mut().for_each(|item| self.name_references(item)),
            _ => {}
        }
    }

    /// 轮值的成员里组织中没有的邮箱，不分大小写地去重，按出现的先后。
    pub fn unknown_members(&self, value: &Value) -> Vec<String> {
        let mut unknown: Vec<String> = Vec::new();

        self.collect_unknown_members(value, &mut unknown);
        unknown
    }

    fn collect_unknown_members(&self, value: &Value, unknown: &mut Vec<String>) {
        match value {
            Value::Object(object) => {
                let members = object.get("members").and_then(Value::as_array);

                for email in members.into_iter().flatten().filter_map(Value::as_str) {
                    let seen = unknown.iter().any(|name| name.eq_ignore_ascii_case(email));

                    if !seen && !self.users.contains(email) {
                        unknown.push(email.to_string());
                    }
                }

                for child in object.values() {
                    self.collect_unknown_members(child, unknown);
                }
            }
            Value::Array(items) => {
                for item in items {
                    self.collect_unknown_members(item, unknown);
                }
            }
            _ => {}
        }
    }

    /// 把名字换回 id；有一个名字对不上就报错。
    pub fn resolve_references(&self, value: &mut Value) -> Result<()> {
        match value {
            Value::Object(object) => {
                self.ids_back(object)?;

                for child in object.values_mut() {
                    self.resolve_references(child)?;
                }

                Ok(())
            }
            Value::Array(items) => items
                .iter_mut()
                .try_for_each(|item| self.resolve_references(item)),
            _ => Ok(()),
        }
    }

    fn ids_back(&self, object: &mut Map<String, Value>) -> Result<()> {
        for (id_key, name_key, kind, what) in KEYS {
            // 名字是字符串的才换：节假日的 `calendar` 键也是装着日历用法的对象。一个对象里同时有 id
            // 和名字时（不该有），以名字为准。
            if let Some(Value::String(name)) = object.get(name_key).cloned() {
                let id = self.names(kind).id_of(what, &name)?;
                object.remove(name_key);
                object.insert(id_key.into(), Value::String(id.0));
            }
        }

        if let Some(Value::Array(members)) = object.get_mut("members") {
            for member in members.iter_mut() {
                if let Value::String(email) = member {
                    *member = Value::String(self.users.id_of("user", email)?.0);
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn directory() -> Directory {
        Directory {
            users: Names::new(
                [(Id::from_string("u1"), "Ann@Example.com".to_string())],
                true,
            ),
            teams: Names::new([(Id::from_string("t1"), "Payments".to_string())], false),
            schedules: Names::new(
                [
                    (Id::from_string("s1"), "Primary".to_string()),
                    (Id::from_string("s2"), "Twice".to_string()),
                    (Id::from_string("s3"), "Twice".to_string()),
                ],
                false,
            ),
            policies: Names::default(),
            calendars: Names::new([(Id::from_string("c1"), "China".to_string())], false),
        }
    }

    #[test]
    fn ids_become_names_and_come_back() {
        let original = json!({
            "steps": [{"targets": [{"kind": "user", "userId": "u1"}, {"kind": "schedule", "scheduleId": "s1"}]}],
            "rotations": [{"members": ["u1"], "calendar": {"calendarId": "c1", "holidays": "exclude"}}],
            "teamId": "t1"
        });
        let mut value = original.clone();

        directory().name_references(&mut value);

        assert_eq!(
            value,
            json!({
                "steps": [{"targets": [{"kind": "user", "user": "Ann@Example.com"}, {"kind": "schedule", "schedule": "Primary"}]}],
                "rotations": [{"members": ["Ann@Example.com"], "calendar": {"calendar": "China", "holidays": "exclude"}}],
                "team": "Payments"
            })
        );

        // Emails match without regard to case.
        value["rotations"][0]["members"][0] = json!("ann@example.com");
        directory().resolve_references(&mut value).unwrap();
        assert_eq!(value, original);
    }

    #[test]
    fn a_name_that_is_missing_or_shared_is_refused() {
        let mut missing = json!({"team": "Nobody"});
        let mut shared = json!({"schedule": "Twice"});

        assert!(directory().resolve_references(&mut missing).is_err());
        assert!(directory().resolve_references(&mut shared).is_err());
    }
}
