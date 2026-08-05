// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 敏感数据脱敏（masking）：纯逻辑 + 写入端口。
//!
//! 模型：每条规则 = `(regex, replacement)`。`replacement` 走 `regex` 的替换语义，
//! 支持 `$1` / `${name}` 捕获组回引（如 `****$1` 保留尾段）；普通串（如 `[REDACTED]`）
//! 当字面量。一条值按规则顺序依次 `replace_all`，前一条的输出喂给后一条。
//!
//! 两条用途共用 [`Masker`]：
//! - **写入端**：[`MaskingProvider::ingest_masker`] 返回该 org「写入即脱敏」规则编译出的
//!   masker，`IngestService` 在 pipeline 之后、落盘之前对事件字符串值就地脱敏（不可逆）。
//! - **查询端**：把全部规则编译进 `mask(col)` UDF，对列即时脱敏（原值仍在底层）。
//!
//! 非法 regex 在编译期静默跳过（写入路径的规则在落库前已校验，这里只是兜底）。

use std::{borrow::Cow, sync::Arc};

use async_trait::async_trait;
use regex::Regex;

use crate::shared::{Result, ids::Id};

/// 一条编译好的脱敏规则。
struct MaskRule {
    regex: Regex,
    replacement: String,
}

/// 一组脱敏规则的编译产物：对字符串 / JSON 值就地应用。
///
/// 规则按传入顺序应用（caller 通常按 name ASC 传入，结果确定）。
#[derive(Default)]
pub struct Masker {
    rules: Vec<MaskRule>,
}

impl Masker {
    /// 编译 `(pattern, replacement)` 列表；非法 regex 跳过。空列表 → 空 masker（no-op）。
    pub fn compile(rules: impl IntoIterator<Item = (String, String)>) -> Self {
        let rules = rules
            .into_iter()
            .filter_map(|(pat, replacement)| {
                Regex::new(&pat)
                    .ok()
                    .map(|regex| MaskRule { regex, replacement })
            })
            .collect();
        Self { rules }
    }

    /// 无任何规则 → caller 应跳过脱敏（零开销）。
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// 对单个字符串依次应用全部规则。无命中时零拷贝返回原借用。
    pub fn mask_str<'a>(&self, input: &'a str) -> Cow<'a, str> {
        if self.rules.is_empty() {
            return Cow::Borrowed(input);
        }
        let mut owned: Option<String> = None;
        for rule in &self.rules {
            // 借用在内层块内结束，随后才可能改写 `owned`。
            let next = {
                let src: &str = owned.as_deref().unwrap_or(input);
                match rule.regex.replace_all(src, rule.replacement.as_str()) {
                    Cow::Owned(s) => Some(s),
                    Cow::Borrowed(_) => None,
                }
            };
            if let Some(s) = next {
                owned = Some(s);
            }
        }
        match owned {
            Some(s) => Cow::Owned(s),
            None => Cow::Borrowed(input),
        }
    }

    /// 递归对 JSON 值里的全部字符串叶子就地脱敏（含嵌套 object / array）。
    pub fn mask_value(&self, value: &mut serde_json::Value) {
        match value {
            serde_json::Value::String(s) => {
                if let Cow::Owned(masked) = self.mask_str(s) {
                    *s = masked;
                }
            }
            serde_json::Value::Array(items) => {
                for it in items {
                    self.mask_value(it);
                }
            }
            serde_json::Value::Object(map) => {
                for v in map.values_mut() {
                    self.mask_value(v);
                }
            }
            _ => {}
        }
    }

    /// 对一组事件字段（`RawEvent.fields`）就地脱敏。
    pub fn mask_fields(&self, fields: &mut serde_json::Map<String, serde_json::Value>) {
        if self.rules.is_empty() {
            return;
        }
        for v in fields.values_mut() {
            self.mask_value(v);
        }
    }
}

/// 写入端口：按 org 返回「写入即脱敏」规则编译出的 [`Masker`]。
///
/// 由 infra 实装（带缓存）；空规则集返回空 masker，`IngestService` 据此跳过。
#[async_trait]
pub trait MaskingProvider: Send + Sync {
    async fn ingest_masker(&self, org_id: &Id) -> Result<Arc<Masker>>;
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn masker(rules: &[(&str, &str)]) -> Masker {
        Masker::compile(rules.iter().map(|(p, r)| (p.to_string(), r.to_string())))
    }

    #[test]
    fn empty_masker_is_noop() {
        let m = Masker::default();
        assert!(m.is_empty());
        assert_eq!(m.mask_str("anything"), "anything");
        // 无命中时返回借用（零拷贝）。
        assert!(matches!(m.mask_str("anything"), Cow::Borrowed(_)));
    }

    #[test]
    fn redacts_matches_with_replacement() {
        let m = masker(&[(r"\d{3}-\d{2}-\d{4}", "[SSN]")]);
        assert_eq!(m.mask_str("ssn 123-45-6789 end"), "ssn [SSN] end");
        // 无命中走借用分支。
        assert!(matches!(m.mask_str("no digits here"), Cow::Borrowed(_)));
    }

    #[test]
    fn supports_capture_group_backref() {
        // 邮箱本地部分打码，保留域名。
        let m = masker(&[(r"[\w.]+@([\w.]+)", "***@$1")]);
        assert_eq!(m.mask_str("alice@example.com"), "***@example.com");
    }

    #[test]
    fn applies_rules_in_sequence() {
        let m = masker(&[(r"foo", "bar"), (r"bar", "baz")]);
        // 第一条 foo→bar，第二条把（含刚产生的）bar→baz。
        assert_eq!(m.mask_str("foo"), "baz");
    }

    #[test]
    fn bad_regex_is_skipped() {
        let m = masker(&[(r"[invalid(", "x"), (r"good", "G")]);
        assert_eq!(m.mask_str("good"), "G");
    }

    #[test]
    fn masks_nested_json_string_leaves() {
        let m = masker(&[(r"secret", "[X]")]);
        let mut v = json!({
            "a": "secret",
            "b": 42,
            "c": ["secret", {"d": "secret"}],
        });
        m.mask_value(&mut v);
        assert_eq!(v, json!({"a": "[X]", "b": 42, "c": ["[X]", {"d": "[X]"}]}));
    }

    #[test]
    fn mask_fields_in_place() {
        let m = masker(&[(r"\bpw=\S+", "pw=[REDACTED]")]);
        let mut fields = json!({"msg": "login pw=hunter2 ok", "n": 1})
            .as_object()
            .unwrap()
            .clone();
        m.mask_fields(&mut fields);
        assert_eq!(fields["msg"], json!("login pw=[REDACTED] ok"));
        assert_eq!(fields["n"], json!(1));
    }
}
