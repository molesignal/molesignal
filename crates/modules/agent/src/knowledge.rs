// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Knowledge: one base for the organization and one for each team.
//!
//! A knowledge base is a set of UTF-8 text files, optionally organized into subdirectories. The
//! entry file [`ENTRY_FILE`] loads automatically in sessions and contains only a directory listing
//! and a one-sentence guide, using `@path` to point to other files; tools read other files as
//! needed. Each file is limited to [`MAX_FILE_BYTES`], and each base is limited to
//! [`MAX_TOTAL_BYTES`] and [`MAX_FILES`] files; files or directories starting with `.` are not
//! accepted. Organization knowledge holds cross-team conventions; team knowledge holds the team's
//! services, on-call handbooks, and upstream and downstream dependencies.

use async_trait::async_trait;
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde::{Deserialize, Serialize};

use crate::scope::Scope;

/// The entry file.
pub const ENTRY_FILE: &str = "MOLE.md";
pub const MAX_FILE_BYTES: usize = 1024 * 1024;
pub const MAX_TOTAL_BYTES: usize = 5 * 1024 * 1024;
pub const MAX_FILES: usize = 200;
pub const MAX_PATH_CHARS: usize = 512;
/// The maximum length of an entry file passed to the model.
const ENTRY_PROMPT_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

/// A file without its content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeFileMeta {
    pub path: String,
    pub size_bytes: i64,
    pub updated_by: Id,
    pub updated_at: TimestampMicros,
}

/// An overview of a knowledge base.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeBase {
    pub scope: Scope,
    pub files: i64,
    pub bytes: i64,
    /// The entry file's last modification time; empty if there is no entry file.
    pub entry_updated_at: Option<TimestampMicros>,
    /// The number of other files changed after the entry file: the entry file may be outdated.
    pub changed_since_entry: i64,
    pub updated_at: Option<TimestampMicros>,
}

/// A line found by search.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeHit {
    pub scope: Scope,
    pub path: String,
    /// Counts from 1.
    pub line: usize,
    pub text: String,
}

/// Validates and normalizes a path: separated by `/`, with no leading `/`, empty segments, `.`,
/// `..`, or segments starting with `.`.
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

/// Checks whether file content is accepted: within the per-file limit and text (no NUL).
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

/// Checks that the knowledge base remains within quota after the write.
/// `files` and `bytes` are the totals after the write.
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

/// Lines containing `query` (case-insensitive); if `query` has multiple words, every word must
/// appear in the line.
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

/// The form in which automatically loaded session entry files are passed to the model.
/// `entries` contains (source description, content) pairs, ordered by organization, then team.
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
    /// Overviews of all knowledge bases in the organization that contain files.
    async fn bases(&self, org_id: &Id) -> Result<Vec<KnowledgeBase>>;
    async fn list(&self, org_id: &Id, scope: &Scope) -> Result<Vec<KnowledgeFileMeta>>;
    async fn get(&self, org_id: &Id, scope: &Scope, path: &str) -> Result<Option<KnowledgeFile>>;
    /// Writes a set of files (creating or replacing them).
    /// Quota is calculated after the write; if exceeded, no files are written.
    async fn put(
        &self,
        org_id: &Id,
        scope: &Scope,
        files: &[(String, String)],
        actor: &Id,
    ) -> Result<()>;
    async fn delete(&self, org_id: &Id, scope: &Scope, path: &str) -> Result<bool>;
    /// Renames or moves a file to another directory; reports a conflict if the destination file
    /// already exists.
    async fn rename(
        &self,
        org_id: &Id,
        scope: &Scope,
        from: &str,
        to: &str,
        actor: &Id,
    ) -> Result<()>;
    /// Searches files in these scopes, returning at most `limit` lines.
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
