// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result};

static CLASS_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(.+?) -> (.+?):$").expect("class mapping regex"));
static METHOD_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:(\d+):(\d+):)?(?:\S+\s+)?([^\s(]+)\([^)]*\)(?::(\d+)(?::(\d+))?)? -> (\S+)$")
        .expect("method mapping regex")
});

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AndroidOriginalFrame {
    pub class_name: String,
    pub function: String,
    pub line: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct AndroidMapping {
    classes: Vec<ClassMapping>,
}

#[derive(Debug, Clone)]
struct ClassMapping {
    original: String,
    obfuscated: String,
    methods: Vec<MethodMapping>,
}

#[derive(Debug, Clone)]
struct MethodMapping {
    original: String,
    obfuscated: String,
    obfuscated_lines: Option<(u32, u32)>,
    original_lines: Option<(u32, u32)>,
}

impl AndroidMapping {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let text = std::str::from_utf8(bytes)
            .map_err(|error| Error::invalid(format!("Android mapping.txt UTF-8: {error}")))?;
        let mut classes: Vec<ClassMapping> = Vec::new();
        for raw_line in text.lines() {
            let line = raw_line.trim_end();
            if line.is_empty() || line.trim_start().starts_with('#') {
                continue;
            }
            if !raw_line.chars().next().is_some_and(char::is_whitespace) {
                if let Some(captures) = CLASS_LINE.captures(line) {
                    classes.push(ClassMapping {
                        original: captures[1].trim().to_string(),
                        obfuscated: captures[2].trim().to_string(),
                        methods: Vec::new(),
                    });
                }
                continue;
            }
            let Some(class) = classes.last_mut() else {
                continue;
            };
            let Some(captures) = METHOD_LINE.captures(line.trim()) else {
                continue;
            };
            class.methods.push(MethodMapping {
                obfuscated_lines: capture_range(&captures, 1, 2),
                original: captures[3].to_string(),
                original_lines: capture_range(&captures, 4, 5),
                obfuscated: captures[6].to_string(),
            });
        }
        if classes.is_empty() {
            return Err(Error::invalid(
                "Android mapping.txt contains no class mappings",
            ));
        }
        Ok(Self { classes })
    }

    pub fn translate(
        &self,
        class_name: &str,
        function: &str,
        line: Option<u32>,
    ) -> Option<AndroidOriginalFrame> {
        let class = self
            .classes
            .iter()
            .find(|candidate| candidate.obfuscated == class_name)?;
        let candidates = class
            .methods
            .iter()
            .filter(|method| method.obfuscated == function);
        let method = if let Some(line) = line {
            candidates
                .clone()
                .find(|method| {
                    method
                        .obfuscated_lines
                        .is_some_and(|(start, end)| (start..=end).contains(&line))
                })
                .or_else(|| {
                    candidates
                        .into_iter()
                        .find(|method| method.obfuscated_lines.is_none())
                })
        } else {
            candidates.into_iter().next()
        };
        let (function, translated_line) = method.map_or_else(
            || (function.to_string(), line),
            |method| {
                (
                    method.original.clone(),
                    translate_line(line, method.obfuscated_lines, method.original_lines),
                )
            },
        );
        Some(AndroidOriginalFrame {
            class_name: class.original.clone(),
            function,
            line: translated_line,
        })
    }
}

fn capture_range(captures: &regex::Captures<'_>, start: usize, end: usize) -> Option<(u32, u32)> {
    let start = captures.get(start)?.as_str().parse().ok()?;
    let end = captures
        .get(end)
        .and_then(|value| value.as_str().parse().ok())
        .unwrap_or(start);
    Some((start, end))
}

fn translate_line(
    line: Option<u32>,
    obfuscated: Option<(u32, u32)>,
    original: Option<(u32, u32)>,
) -> Option<u32> {
    let (line, (obfuscated_start, _), (original_start, original_end)) =
        (line?, obfuscated?, original?);
    Some(
        original_start.saturating_add(
            line.saturating_sub(obfuscated_start)
                .min(original_end.saturating_sub(original_start)),
        ),
    )
}

pub fn validate(bytes: &[u8]) -> Result<()> {
    AndroidMapping::parse(bytes).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_class_method_and_line() {
        let mapping = AndroidMapping::parse(
            b"com.example.Checkout -> a:\n    1:3:void submit(java.lang.String):40:42 -> b\n",
        )
        .expect("mapping");
        assert_eq!(
            mapping.translate("a", "b", Some(2)),
            Some(AndroidOriginalFrame {
                class_name: "com.example.Checkout".into(),
                function: "submit".into(),
                line: Some(41),
            })
        );
    }
}
