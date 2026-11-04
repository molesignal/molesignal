// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 升级策略、排班、路由规则的导入导出（告警规则另见 [`super::rule_transfer`]）。
//!
//! 导出成按名称引用的包（[`Directory`]）：不带 id、组织和时间，排班不带替班、换班和临时覆盖。
//! 导入先预演：每一项按名称对上已有的——没有就新建，正好一个就比较字段，同名的有几个、或包里同一个
//! 名称写了两次就报错；也可以只新建。每一项照保存时那样检查，整包都说得通才写，按包里的顺序逐个写。
//!
//! 排班还能从 PagerDuty、Opsgenie 的 API 响应导入，转换后走同一条流程（见 [`foreign`]）。

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;
use serde_json::Value;

use super::rule_transfer::{ImportAction, MAX_IMPORT_BYTES};
use crate::shared::{Error, Result, ids::Id};

pub mod directory;
pub mod foreign;
pub mod schedules;

pub use directory::{Directory, Names};

pub const POLICY_FORMAT: &str = "molesignal.escalation-policies/v1";
pub const SCHEDULE_FORMAT: &str = "molesignal.schedules/v1";
pub const ROUTING_FORMAT: &str = "molesignal.routing-rules/v1";

/// 一次最多导入几项。
pub const MAX_IMPORT_ITEMS: usize = 500;

/// 导入的一项会怎样。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferItem {
    pub name: String,
    pub action: ImportAction,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// 新建或更新的那一项。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,
    /// 组织里没有的成员邮箱，一个不落。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unknown_members: Vec<String>,
    /// 带不过来、会改变结果的内容，每一条都挡住写入。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blockers: Vec<Finding>,
    /// 明确跳过的内容，以及和来源对照的结果。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<Finding>,
}

impl TransferItem {
    fn new(name: &str, action: ImportAction) -> Self {
        Self {
            name: name.to_string(),
            action,
            changes: Vec::new(),
            error: None,
            id: None,
            unknown_members: Vec::new(),
            blockers: Vec::new(),
            findings: Vec::new(),
        }
    }

    fn failed(name: &str, error: impl Into<String>) -> Self {
        Self {
            error: Some(error.into()),
            ..Self::new(name, ImportAction::Error)
        }
    }

    fn skipped(name: &str) -> Self {
        Self::new(name, ImportAction::Skip)
    }
}

/// 预演里的一条说明：`code` 是界面文案的键，`params` 是文案里的值。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub code: &'static str,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<&'static str, String>,
}

impl Finding {
    pub fn new(code: &'static str) -> Self {
        Self {
            code,
            params: BTreeMap::new(),
        }
    }

    pub fn with(mut self, key: &'static str, value: impl Into<String>) -> Self {
        self.params.insert(key, value.into());
        self
    }
}

/// 预演或导入的结果。
#[derive(Debug, Clone, Serialize)]
pub struct TransferReport {
    pub format: &'static str,
    pub items: Vec<TransferItem>,
    /// 写进去了没有：预演，或有一项出错时都没写。
    pub applied: bool,
    /// 说整个文件的说明。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<Finding>,
}

/// 一项要写的：新建（没有 id）或更新哪一个，以及它在报告里是第几项（写完填上 id）。
pub struct Write<T> {
    pub existing: Option<Id>,
    pub value: T,
    pub item: usize,
}

/// 读一个文件：JSON 或 YAML。
pub fn read_content(content: &str) -> Result<Value> {
    if content.len() > MAX_IMPORT_BYTES {
        return Err(Error::invalid("the file is larger than 4 MiB"));
    }

    serde_json::from_str(content)
        .or_else(|_| serde_yaml::from_str(content))
        .map_err(|_| Error::invalid("the content is neither JSON nor YAML"))
}

/// 读一个包：JSON 或 YAML，`format` 对得上，`list_key` 下是一组对象。
pub fn read_bundle(content: &str, format: &'static str, list_key: &str) -> Result<Vec<Value>> {
    bundle_items(&read_content(content)?, format, list_key)
}

/// 包里的那一组对象。
pub fn bundle_items(bundle: &Value, format: &'static str, list_key: &str) -> Result<Vec<Value>> {
    if bundle.get("format").and_then(Value::as_str) != Some(format) {
        return Err(Error::invalid(format!("this is not a `{format}` bundle")));
    }

    let items = bundle
        .get(list_key)
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| Error::invalid(format!("the bundle has no `{list_key}` list")))?;

    if items.len() > MAX_IMPORT_ITEMS {
        return Err(Error::invalid(format!(
            "at most {MAX_IMPORT_ITEMS} items in one import"
        )));
    }

    Ok(items)
}

/// 一项的可移植 JSON：去掉 `drop` 里的键，id 换成名字。
pub fn portable_of<T: Serialize>(item: &T, drop: &[&str], directory: &Directory) -> Value {
    let mut value = serde_json::to_value(item).unwrap_or(Value::Null);

    if let Value::Object(object) = &mut value {
        for key in drop {
            object.remove(*key);
        }
    }

    directory.name_references(&mut value);
    value
}

/// 两份可移植 JSON 顶层哪些键不同。
pub fn changed_fields(before: &Value, after: &Value) -> Vec<String> {
    let empty = serde_json::Map::new();
    let before = before.as_object().unwrap_or(&empty);
    let after = after.as_object().unwrap_or(&empty);
    let mut keys: Vec<&String> = before.keys().chain(after.keys()).collect();

    keys.sort();
    keys.dedup();
    keys.into_iter()
        .filter(|key| before.get(*key) != after.get(*key))
        .cloned()
        .collect()
}

/// 已有的一项：id、名称、可移植 JSON。
pub struct Existing {
    pub id: Id,
    pub name: String,
    pub portable: Value,
}

/// 预演整包。`build` 把换回 id 的一项变成要写的值并照保存时那样检查（`Some` 是要更新的那一项）；
/// `portable` 把写的值再换成可移植 JSON，和已有的比。
pub fn plan<T>(
    incoming: Vec<Value>,
    existing: &[Existing],
    directory: &Directory,
    create_only: bool,
    build: impl Fn(Value, Option<&Id>) -> Result<T>,
    portable: impl Fn(&T) -> Value,
) -> (Vec<TransferItem>, Vec<Write<T>>) {
    let mut names: HashMap<String, usize> = HashMap::new();

    for value in &incoming {
        *names.entry(name_of(value)).or_default() += 1;
    }

    let mut items = Vec::new();
    let mut writes = Vec::new();

    for mut value in incoming {
        let name = name_of(&value);

        if name.is_empty() {
            items.push(TransferItem::failed(&name, "every item needs a name"));
            continue;
        }

        if names.get(&name).copied().unwrap_or_default() > 1 {
            items.push(TransferItem::failed(
                &name,
                "the import names this item more than once",
            ));
            continue;
        }

        let matches: Vec<&Existing> = existing
            .iter()
            .filter(|item| item.name.trim() == name)
            .collect();

        if matches.len() > 1 {
            items.push(TransferItem::failed(
                &name,
                format!("{} existing items share this name", matches.len()),
            ));
            continue;
        }

        let current = matches.first().copied();

        if current.is_some() && create_only {
            items.push(TransferItem {
                id: current.map(|item| item.id.clone()),
                ..TransferItem::skipped(&name)
            });
            continue;
        }

        let unknown_members = directory.unknown_members(&value);

        if !unknown_members.is_empty() {
            items.push(TransferItem {
                unknown_members,
                ..TransferItem::failed(&name, "some members have no account in this organization")
            });
            continue;
        }

        let built = directory
            .resolve_references(&mut value)
            .and_then(|()| build(value, current.map(|item| &item.id)));

        let built = match built {
            Ok(built) => built,
            Err(error) => {
                items.push(TransferItem::failed(&name, error.to_string()));
                continue;
            }
        };

        let after = portable(&built);
        let changes = current.map(|item| changed_fields(&item.portable, &after));
        let action = match &changes {
            None => ImportAction::Create,
            Some(changes) if changes.is_empty() => ImportAction::Unchanged,
            Some(_) => ImportAction::Update,
        };

        items.push(TransferItem {
            changes: changes.unwrap_or_default(),
            id: current.map(|item| item.id.clone()),
            ..TransferItem::new(&name, action)
        });

        if action != ImportAction::Unchanged {
            writes.push(Write {
                existing: current.map(|item| item.id.clone()),
                value: built,
                item: items.len() - 1,
            });
        }
    }

    (items, writes)
}

fn name_of(value: &Value) -> String {
    value
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_bundle_is_planned_item_by_item_by_name() {
        let directory = Directory::default();
        let existing = vec![
            Existing {
                id: Id::from_string("1"),
                name: "same".into(),
                portable: json!({"name": "same", "repeat": false}),
            },
            Existing {
                id: Id::from_string("2"),
                name: "changed".into(),
                portable: json!({"name": "changed", "repeat": false}),
            },
            Existing {
                id: Id::from_string("3"),
                name: "dup".into(),
                portable: json!({"name": "dup"}),
            },
            Existing {
                id: Id::from_string("4"),
                name: "dup".into(),
                portable: json!({"name": "dup"}),
            },
        ];
        let incoming = vec![
            json!({"name": "same", "repeat": false}),
            json!({"name": "changed", "repeat": true}),
            json!({"name": "new", "repeat": false}),
            json!({"name": "dup"}),
            json!({"name": "twice"}),
            json!({"name": "twice"}),
            json!({"name": "bad", "repeat": "no"}),
        ];

        let (items, writes) = plan(
            incoming,
            &existing,
            &directory,
            false,
            |value, _| {
                value
                    .get("repeat")
                    .map_or(Some(false), Value::as_bool)
                    .map(|repeat| json!({"name": value["name"], "repeat": repeat}))
                    .ok_or_else(|| Error::invalid("repeat is a boolean"))
            },
            Clone::clone,
        );

        assert_eq!(
            items
                .iter()
                .map(|item| (item.name.as_str(), item.action))
                .collect::<Vec<_>>(),
            vec![
                ("same", ImportAction::Unchanged),
                ("changed", ImportAction::Update),
                ("new", ImportAction::Create),
                ("dup", ImportAction::Error),
                ("twice", ImportAction::Error),
                ("twice", ImportAction::Error),
                ("bad", ImportAction::Error),
            ]
        );
        assert_eq!(items[1].changes, vec!["repeat".to_string()]);
        assert_eq!(writes.len(), 2);
        assert_eq!(writes[0].existing, Some(Id::from_string("2")));
        assert!(writes[1].existing.is_none());
    }

    #[test]
    fn every_unknown_member_is_named() {
        let directory = Directory {
            users: Names::new(
                [(Id::from_string("u1"), "ann@example.com".to_string())],
                true,
            ),
            ..Directory::default()
        };
        let incoming = vec![json!({
            "name": "nights",
            "rotations": [
                {"members": ["ann@example.com", "bob@example.com"]},
                {"members": ["cid@example.com", "BOB@example.com"]}
            ]
        })];

        let (items, writes) = plan(
            incoming,
            &[],
            &directory,
            false,
            |value, _| Ok(value),
            Clone::clone,
        );

        assert_eq!(items[0].action, ImportAction::Error);
        assert_eq!(
            items[0].unknown_members,
            vec!["bob@example.com".to_string(), "cid@example.com".to_string()]
        );
        assert!(writes.is_empty());
    }

    #[test]
    fn only_bundles_of_the_right_format_are_read() {
        let content = r#"{"format": "molesignal.schedules/v1", "schedules": [{"name": "a"}]}"#;

        assert_eq!(
            read_bundle(content, SCHEDULE_FORMAT, "schedules")
                .unwrap()
                .len(),
            1
        );
        assert!(read_bundle(content, POLICY_FORMAT, "policies").is_err());
        assert!(
            read_bundle(
                "format: molesignal.schedules/v1\nschedules:\n  - name: b\n",
                SCHEDULE_FORMAT,
                "schedules"
            )
            .is_ok()
        );
    }
}
