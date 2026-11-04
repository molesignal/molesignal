// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What people say about a recorded session: a comment at a moment of its replay. A comment belongs
//! to the session of one application and is seen by everyone who may watch the replay.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

/// Longest comment, in characters.
pub const MAX_COMMENT_LEN: usize = 2_000;
/// The most comments one session holds.
pub const MAX_COMMENTS_PER_SESSION: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RumReplayComment {
    pub org_id: Id,
    pub id: String,
    pub application_id: String,
    pub session_id: String,
    /// The moment of the session it is about, in microseconds since the epoch.
    pub at_time: i64,
    pub body: String,
    pub author_id: Option<Id>,
    /// Who wrote it, as the directory names them when it is read.
    pub author_name: Option<String>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

/// The text of a comment as it is kept: what is around it taken off, not empty, not long.
pub fn validate_comment_body(body: &str) -> Result<&str> {
    let body = body.trim();
    let length = body.chars().count();

    if length == 0 || length > MAX_COMMENT_LEN || body.contains('\0') {
        return Err(Error::invalid(format!(
            "a comment must contain between 1 and {MAX_COMMENT_LEN} characters"
        )));
    }

    Ok(body)
}

#[async_trait]
pub trait RumReplayCommentRepository: Send + Sync {
    /// Keeps a comment, within what a session holds.
    async fn create(&self, comment: RumReplayComment) -> Result<RumReplayComment>;

    /// The comments of a session, earliest moment first.
    async fn list(&self, org_id: &Id, session_id: &str) -> Result<Vec<RumReplayComment>>;

    /// Not found is `Error::NotFound`.
    async fn get(&self, org_id: &Id, session_id: &str, id: &str) -> Result<RumReplayComment>;

    async fn update_body(
        &self,
        org_id: &Id,
        session_id: &str,
        id: &str,
        body: &str,
        now: TimestampMicros,
    ) -> Result<RumReplayComment>;

    async fn delete(&self, org_id: &Id, session_id: &str, id: &str) -> Result<()>;

    /// Removes up to `limit` comments on moments older than `cutoff`, as the replays they are about
    /// go, and says how many.
    async fn delete_older_than(&self, cutoff: TimestampMicros, limit: usize) -> Result<usize>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_comment_is_not_empty_and_not_long() {
        assert_eq!(validate_comment_body("  why here?  ").unwrap(), "why here?");
        assert!(validate_comment_body("   ").is_err());
        assert!(validate_comment_body(&"字".repeat(MAX_COMMENT_LEN)).is_ok());
        assert!(validate_comment_body(&"字".repeat(MAX_COMMENT_LEN + 1)).is_err());
        assert!(validate_comment_body("a\0b").is_err());
    }
}
