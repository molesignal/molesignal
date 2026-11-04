// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Reading the audit trail (`_audit`) with no caller to ask: the runs of a scheduled pipeline and
//! its backfills. They read with the access of a person, the one who last decided what the pipeline
//! reads and where that goes, or who started the backfill, and every read checks that they still
//! hold `audit.read`. When they do not, the read fails and says who and why: the pipeline's position
//! stays where it is, and it goes on from there once they hold it again or someone who holds it
//! saves the pipeline.

use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    app::iam::{IamAccessService, IamService, member::member_context},
    domain::stream::AUDIT_STREAM,
    infra::query::grants::{AUDIT_READ, ReadGrants},
    shared::{Error, Result, ids::Id},
};

#[async_trait]
pub trait AuditReadCheck: Send + Sync {
    /// Whether the person holds `audit.read` in the organization now. [`Error::Forbidden`] says who
    /// and why not; any other error is IAM failing, nothing to do with them.
    async fn check(&self, org_id: &Id, user_id: &Id) -> Result<()>;
}

/// The check against IAM: their account, the organization, their membership and their roles.
pub struct IamAuditRead {
    iam: Arc<IamService>,
    access: Arc<IamAccessService>,
}

impl IamAuditRead {
    pub fn new(iam: Arc<IamService>, access: Arc<IamAccessService>) -> Self {
        Self { iam, access }
    }
}

#[async_trait]
impl AuditReadCheck for IamAuditRead {
    async fn check(&self, org_id: &Id, user_id: &Id) -> Result<()> {
        let member = member_context(&self.iam, &self.access, org_id, user_id, refused).await?;

        if member.context.has_permission(AUDIT_READ) {
            return Ok(());
        }

        Err(refused(&member.email, "no longer holds audit.read"))
    }
}

fn refused(who: &str, why: &str) -> Error {
    Error::forbidden(format!(
        "`_audit` is read with the access of {who}, who {why}. Someone who holds audit.read has to \
         save the pipeline, or start the backfill, again"
    ))
}

/// What a read of `source` may reach, with `reader` the person it reads as. Only the audit trail
/// asks anything of them.
pub async fn grants_to_read(
    check: Option<&dyn AuditReadCheck>,
    org_id: &Id,
    reader: Option<&Id>,
    source: &str,
) -> Result<ReadGrants> {
    if source != AUDIT_STREAM {
        return Ok(ReadGrants::default());
    }

    let (Some(check), Some(reader)) = (check, reader) else {
        return Err(Error::forbidden(
            "`_audit` is read with the access of the person who saved the pipeline, and it has \
             none. Someone who holds audit.read has to save it again",
        ));
    };

    check.check(org_id, reader).await?;

    Ok(ReadGrants::of_caller(true))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Holds `audit.read` for the one person named, and is down for anyone called `down`.
    struct Holder(&'static str);

    #[async_trait]
    impl AuditReadCheck for Holder {
        async fn check(&self, _org_id: &Id, user_id: &Id) -> Result<()> {
            match user_id.0.as_str() {
                "down" => Err(Error::unavailable("iam unreachable")),
                who if who == self.0 => Ok(()),
                who => Err(refused(who, "no longer holds audit.read")),
            }
        }
    }

    async fn grants(reader: Option<&str>, source: &str) -> Result<ReadGrants> {
        let org = Id::from_string("org");
        let reader = reader.map(Id::from_string);

        grants_to_read(Some(&Holder("ada")), &org, reader.as_ref(), source).await
    }

    #[tokio::test]
    async fn the_audit_trail_is_read_while_the_person_still_holds_the_permission() {
        assert!(grants(Some("ada"), AUDIT_STREAM).await.unwrap().audit);
    }

    #[tokio::test]
    async fn a_person_who_lost_the_permission_stops_the_read_and_is_named() {
        let error = grants(Some("bob"), AUDIT_STREAM).await.err().unwrap();

        assert!(matches!(error, Error::Forbidden(_)));
        assert!(error.to_string().contains("bob"), "{error}");
    }

    #[tokio::test]
    async fn nobody_to_read_as_stops_the_read() {
        let error = grants(None, AUDIT_STREAM).await.err().unwrap();

        assert!(matches!(error, Error::Forbidden(_)));

        let org = Id::from_string("org");
        let ada = Id::from_string("ada");

        assert!(
            grants_to_read(None, &org, Some(&ada), AUDIT_STREAM)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn iam_failing_is_not_taken_for_the_person_losing_it() {
        let error = grants(Some("down"), AUDIT_STREAM).await.err().unwrap();

        assert!(!matches!(error, Error::Forbidden(_)));
    }

    #[tokio::test]
    async fn any_other_stream_asks_nothing_of_anyone() {
        assert!(!grants(None, "app_logs").await.unwrap().audit);
        assert!(!grants(Some("bob"), "app_logs").await.unwrap().audit);
    }
}
