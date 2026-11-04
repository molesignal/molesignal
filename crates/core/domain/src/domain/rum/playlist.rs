// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Playlists of RUM sessions: a named list of the sessions a team keeps to go through one after
//! the other, each with the moment worth opening it at. A playlist belongs to one application and
//! is shared by everyone who may read RUM.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

pub const MAX_PLAYLIST_NAME_LEN: usize = 128;
pub const MAX_PLAYLIST_DESCRIPTION_LEN: usize = 1_000;
/// The most sessions a playlist holds.
pub const MAX_PLAYLIST_SESSIONS: usize = 500;
const MAX_SESSION_ID_LEN: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RumPlaylist {
    pub org_id: Id,
    pub id: String,
    pub application_id: String,
    pub name: String,
    pub description: String,
    pub created_by: Option<Id>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
    /// How many sessions it holds; counted where the playlist is read.
    pub session_count: u32,
}

/// One session of a playlist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RumPlaylistEntry {
    pub session_id: String,
    /// The moment to open the session at, in microseconds since the epoch; none opens it at its
    /// beginning.
    pub at_time: Option<i64>,
    pub added_by: Option<Id>,
    pub added_at: TimestampMicros,
}

/// What a change of a playlist names. The part it leaves out keeps its value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RumPlaylistPatch {
    pub name: Option<String>,
    pub description: Option<String>,
}

impl RumPlaylistPatch {
    pub fn validate(&self) -> Result<()> {
        if let Some(name) = &self.name {
            validate_playlist_name(name)?;
        }

        if let Some(description) = &self.description {
            validate_playlist_description(description)?;
        }

        Ok(())
    }
}

pub fn validate_playlist_name(name: &str) -> Result<()> {
    let length = name.trim().chars().count();

    if length == 0 || length > MAX_PLAYLIST_NAME_LEN {
        return Err(Error::invalid(format!(
            "playlist name must contain between 1 and {MAX_PLAYLIST_NAME_LEN} characters"
        )));
    }

    Ok(())
}

pub fn validate_playlist_description(description: &str) -> Result<()> {
    if description.chars().count() > MAX_PLAYLIST_DESCRIPTION_LEN {
        return Err(Error::invalid(format!(
            "playlist description must not be longer than {MAX_PLAYLIST_DESCRIPTION_LEN} characters"
        )));
    }

    Ok(())
}

/// The id of a session as a playlist keeps it: the text an SDK made, not empty, not long.
pub fn validate_playlist_session(session_id: &str) -> Result<&str> {
    let session_id = session_id.trim();

    if session_id.is_empty() || session_id.len() > MAX_SESSION_ID_LEN {
        return Err(Error::invalid(format!(
            "session id must contain between 1 and {MAX_SESSION_ID_LEN} characters"
        )));
    }

    Ok(session_id)
}

#[async_trait]
pub trait RumPlaylistRepository: Send + Sync {
    /// Creates the playlist with the sessions it starts with. A name an application already has
    /// is a conflict.
    async fn create(
        &self,
        playlist: RumPlaylist,
        entries: Vec<RumPlaylistEntry>,
    ) -> Result<RumPlaylist>;

    /// Not found is `Error::NotFound`.
    async fn get(&self, org_id: &Id, id: &str) -> Result<RumPlaylist>;

    /// The playlists of the organization, or of one application, the one changed last first.
    async fn list(&self, org_id: &Id, application_id: Option<&str>) -> Result<Vec<RumPlaylist>>;

    async fn update(&self, org_id: &Id, id: &str, patch: RumPlaylistPatch) -> Result<RumPlaylist>;

    async fn delete(&self, org_id: &Id, id: &str) -> Result<()>;

    /// The playlists a session is in, the one changed last first.
    async fn of_session(&self, org_id: &Id, session_id: &str) -> Result<Vec<RumPlaylist>>;

    /// The sessions of a playlist in the order they were added.
    async fn entries(&self, org_id: &Id, id: &str) -> Result<Vec<RumPlaylistEntry>>;

    /// Adds sessions. One that is already in the playlist takes the moment it is given and keeps
    /// its place. A playlist holds at most [`MAX_PLAYLIST_SESSIONS`].
    async fn add(&self, org_id: &Id, id: &str, entries: Vec<RumPlaylistEntry>) -> Result<()>;

    /// Takes a session out; one that is not there is not an error.
    async fn remove(&self, org_id: &Id, id: &str, session_id: &str) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_has_a_length_and_a_description_a_limit() {
        assert!(validate_playlist_name("Checkout failures").is_ok());
        assert!(validate_playlist_name("   ").is_err());
        assert!(validate_playlist_name(&"a".repeat(MAX_PLAYLIST_NAME_LEN + 1)).is_err());
        assert!(validate_playlist_name(&"日".repeat(MAX_PLAYLIST_NAME_LEN)).is_ok());

        assert!(validate_playlist_description("").is_ok());
        assert!(validate_playlist_description(&"a".repeat(MAX_PLAYLIST_DESCRIPTION_LEN)).is_ok());
        assert!(
            validate_playlist_description(&"a".repeat(MAX_PLAYLIST_DESCRIPTION_LEN + 1)).is_err()
        );
    }

    #[test]
    fn a_change_is_checked_for_what_it_names() {
        assert!(RumPlaylistPatch::default().validate().is_ok());

        let named = RumPlaylistPatch {
            name: Some(" ".into()),
            ..Default::default()
        };

        assert!(named.validate().is_err());
    }

    #[test]
    fn a_session_is_told_by_the_id_an_sdk_made() {
        assert_eq!(validate_playlist_session(" abc-1 ").unwrap(), "abc-1");
        assert!(validate_playlist_session("").is_err());
        assert!(validate_playlist_session(&"s".repeat(129)).is_err());
    }
}
