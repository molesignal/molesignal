// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The audit trail (`_audit`) among the streams: there only for a caller who may read it
//! (`audit.read`, as on the audit pages). Another caller finds it neither in the list nor in the
//! runtime figures, and cannot read, change or delete it by its ID.

use crate::{
    app::iam::IamContext,
    domain::stream::{AUDIT_STREAM, StreamDefinition},
    infra::query::grants::AUDIT_READ,
    shared::{Error, Result},
};

/// Whether the caller sees the stream of this name.
pub(super) fn shows(ctx: &IamContext, name: &str) -> bool {
    visible(name, ctx.has_permission(AUDIT_READ))
}

/// Refuses a stream the caller does not see, named by its ID.
pub(super) fn refuse_unseen(ctx: &IamContext, stream: &StreamDefinition) -> Result<()> {
    if shows(ctx, &stream.name) {
        return Ok(());
    }

    Err(Error::forbidden("`_audit` needs the audit.read permission"))
}

fn visible(name: &str, may_read_audit: bool) -> bool {
    name != AUDIT_STREAM || may_read_audit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_audit_trail_shows_only_with_the_audit_grant() {
        assert!(visible("app_logs", false));
        assert!(visible("_synthetic_results", false));
        assert!(!visible(AUDIT_STREAM, false));
        assert!(visible(AUDIT_STREAM, true));
    }
}
