// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 知识：组织一份，每个团队一份。
//!
//! 一份知识是一组 UTF-8 文本文件，可以分子目录。入口文件 [`ENTRY_FILE`] 在会话里自动加载，只放
//! 目录与一句话导引，用 `@路径` 指向其他文件；其他文件在需要时用工具读。单个文件不超过
//! [`MAX_FILE_BYTES`]，一份合计不超过 [`MAX_TOTAL_BYTES`]、[`MAX_FILES`] 个文件；不收以 `.` 开头的
//! 文件或目录。组织知识放跨团队的约定，团队知识放本队的服务、值班手册与上下游。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    agent::scope::Scope,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 入口文件。
pub const ENTRY_FILE: &str = "MOLE.md";
pub const MAX_FILE_BYTES: usize = 1024 * 1024;
pub const MAX_TOTAL_BYTES: usize = 5 * 1024 * 1024;
pub const MAX_FILES: usize = 200;
pub const MAX_PATH_CHARS: usize = 512;
/// 一个入口文件交给模型时最多这么长。
const ENTRY_PROMPT_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeFile {
    pub org_id: Id,
    pub scope: Scope,
    pub path: String,
    pub content: String,
    pub size_bytes: i64,
    pub content_hash: String,
    pub created_by: Id,
    pub updated_by: Id,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

/// 不带内容的文件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeFileMeta {
    pub path: String,
    pub size_bytes: i64,
    pub updated_by: Id,
    pub updated_at: TimestampMicros,
}

/// 一份知识的概况。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeBase {
    pub scope: Scope,
    pub files: i64,
    pub bytes: i64,
    /// 入口文件最后一次修改；没有入口文件时为空。
    pub entry_updated_at: Option<TimestampMicros>,
    /// 入口文件之后又改过的其他文件数：入口文件可能过期了。
    pub changed_since_entry: i64,
    pub updated_at: Option<TimestampMicros>,
}

/// 搜到的一行。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeHit {
    pub scope: Scope,
    pub path: String,
    /// 从 1 数起。
    pub line: usize,
    pub text: String,
}

/// 检查并规整一个路径：`/` 分隔、不以 `/` 开头，没有空段、`.`、`..`，也没有以 `.` 开头的段。
pub fn normalize_path(raw: &str) -> Result<String> {
    let path = raw.trim().trim_start_matches("./").replace('\\', "/");
    let path = path.trim_matches('/');

    if path.is_empty() {
        return Err(Error::invalid("a knowledge file needs a path"));
    }

    if path.chars().count() > MAX_PATH_CHARS {
        return Err(Error::invalid(format!(
            "a knowledge path is at most {MAX_PATH_CHARS} characters"
        )));
    }

    for segment in path.split('/') {
        if segment.is_empty() || segment.starts_with('.') {
            return Err(Error::invalid(format!(
                "`{raw}` is not a knowledge path: no empty segments and nothing starting with a dot"
            )));
        }

        if segment.chars().any(|c| c.is_control()) {
            return Err(Error::invalid(
                "a knowledge path cannot hold control characters",
            ));
        }
    }

    Ok(path.to_string())
}

/// 一个文件的内容能不能收：不超过单文件上限，是文本（没有 NUL）。
pub fn check_content(content: &str) -> Result<()> {
    if content.len() > MAX_FILE_BYTES {
        return Err(Error::invalid(format!(
            "a knowledge file is at most {} KiB",
            MAX_FILE_BYTES / 1024
        )));
    }

    if content.contains('\0') {
        return Err(Error::invalid("a knowledge file must be text"));
    }

    Ok(())
}

/// 写完之后这份知识还在份额里。`files` 与 `bytes` 是写完之后的合计。
pub fn check_quota(files: usize, bytes: usize) -> Result<()> {
    if files > MAX_FILES {
        return Err(Error::invalid(format!(
            "a knowledge base holds at most {MAX_FILES} files"
        )));
    }

    if bytes > MAX_TOTAL_BYTES {
        return Err(Error::invalid(format!(
            "a knowledge base holds at most {} MiB",
            MAX_TOTAL_BYTES / 1024 / 1024
        )));
    }

    Ok(())
}

/// 内容里含有 `query` 的行（不分大小写）；`query` 有几个词时每个词都要在这一行里。
pub fn matching_lines(content: &str, query: &str, limit: usize) -> Vec<(usize, String)> {
    let words = query
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>();

    if words.is_empty() {
        return Vec::new();
    }

    content
        .lines()
        .enumerate()
        .filter(|(_, line)| {
            let line = line.to_lowercase();
            words.iter().all(|word| line.contains(word.as_str()))
        })
        .take(limit)
        .map(|(number, line)| (number + 1, line.chars().take(300).collect()))
        .collect()
}

/// 会话自动加载的入口文件交给模型时的样子。`entries` 是（来源说明, 内容），按组织、团队的顺序。
pub fn entry_prompt(entries: &[(String, String)]) -> Option<String> {
    if entries.is_empty() {
        return None;
    }

    let mut text = String::from(
        "Knowledge written by this organization for you. Each block below is the entry file MOLE.md of one knowledge base. Paths written as @path are other files of the same base: read them with read_knowledge, find things with search_knowledge, and list a base with list_knowledge. Prefer this knowledge to guessing, and check facts that may have changed with your other tools.",
    );

    for (source, content) in entries {
        let mut end = content.len().min(ENTRY_PROMPT_BYTES);

        while !content.is_char_boundary(end) {
            end -= 1;
        }

        let cut = if end < content.len() {
            "\n[the rest of MOLE.md is cut; read it with read_knowledge]"
        } else {
            ""
        };

        text.push_str(&format!(
            "\n\n<knowledge source=\"{source}\">\n{}{cut}\n</knowledge>",
            &content[..end]
        ));
    }

    Some(text)
}

#[async_trait]
pub trait KnowledgeStore: Send + Sync {
    /// 组织里有文件的每份知识的概况。
    async fn bases(&self, org_id: &Id) -> Result<Vec<KnowledgeBase>>;
    async fn list(&self, org_id: &Id, scope: &Scope) -> Result<Vec<KnowledgeFileMeta>>;
    async fn get(&self, org_id: &Id, scope: &Scope, path: &str) -> Result<Option<KnowledgeFile>>;
    /// 写入一组文件（新建或替换）。份额按写完之后算，超出时一个都不写。
    async fn put(
        &self,
        org_id: &Id,
        scope: &Scope,
        files: &[(String, String)],
        actor: &Id,
    ) -> Result<()>;
    async fn delete(&self, org_id: &Id, scope: &Scope, path: &str) -> Result<bool>;
    /// 改名或移到别的目录；目标已有文件时报冲突。
    async fn rename(
        &self,
        org_id: &Id,
        scope: &Scope,
        from: &str,
        to: &str,
        actor: &Id,
    ) -> Result<()>;
    /// 在这些作用域的文件里找，最多 `limit` 行。
    async fn search(
        &self,
        org_id: &Id,
        scopes: &[Scope],
        query: &str,
        limit: usize,
    ) -> Result<Vec<KnowledgeHit>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_relative_and_never_hidden() {
        assert_eq!(
            normalize_path("/runbooks/payments.md").unwrap(),
            "runbooks/payments.md"
        );
        assert_eq!(normalize_path("./MOLE.md").unwrap(), "MOLE.md");
        assert_eq!(normalize_path("a\\b.md").unwrap(), "a/b.md");
        assert!(normalize_path("../etc/passwd").is_err());
        assert!(normalize_path("runbooks/.secret").is_err());
        assert!(normalize_path(".git/config").is_err());
        assert!(normalize_path("a//b").is_err());
        assert!(normalize_path("  ").is_err());
    }

    #[test]
    fn content_and_quota_have_limits() {
        assert!(check_content("ok").is_ok());
        assert!(check_content("a\0b").is_err());
        assert!(check_content(&"x".repeat(MAX_FILE_BYTES + 1)).is_err());
        assert!(check_quota(MAX_FILES, MAX_TOTAL_BYTES).is_ok());
        assert!(check_quota(MAX_FILES + 1, 0).is_err());
        assert!(check_quota(1, MAX_TOTAL_BYTES + 1).is_err());
    }

    #[test]
    fn every_word_must_be_on_the_line() {
        let content = "payments uses postgres\ncheckout calls Payments over gRPC\nnothing here";

        let hits = matching_lines(content, "payments grpc", 10);
        assert_eq!(
            hits,
            vec![(2, "checkout calls Payments over gRPC".to_string())]
        );
        assert_eq!(matching_lines(content, "payments", 1).len(), 1);
        assert!(matching_lines(content, " ", 10).is_empty());
    }

    #[test]
    fn entry_files_are_wrapped_and_long_ones_cut() {
        assert!(entry_prompt(&[]).is_none());

        let long = "目".repeat(ENTRY_PROMPT_BYTES);
        let text = entry_prompt(&[
            ("organization".into(), "# Services\n@services.md".into()),
            ("team Platform".into(), long),
        ])
        .unwrap();

        assert!(text.contains("<knowledge source=\"organization\">\n# Services"));
        assert!(text.contains("the rest of MOLE.md is cut"));
    }
}
