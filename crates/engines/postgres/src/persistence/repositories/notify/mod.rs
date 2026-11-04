// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

mod calls;
mod connectors;
mod defaults;
mod deliveries;
mod endpoints;
mod events;
mod follow_ups;
mod policies;
mod preferences;
mod replies;
mod templates;

pub use calls::PgNotifyCallRepository;
pub use connectors::PgNotifyConnectorRepository;
pub use defaults::{
    PgNotifyRouteReferenceRepository, PgOrganizationNotifyDefaultRepository,
    PgTeamNotifyDefaultRepository,
};
pub use deliveries::PgNotifyDeliveryRepository;
pub use endpoints::PgUserNotifyEndpointRepository;
pub use events::PgNotifyEventRepository;
pub use follow_ups::PgNotifyFollowUpRepository;
pub use policies::PgNotifyPolicyRepository;
pub use preferences::PgUserNotifyPreferenceRepository;
pub use replies::PgNotifyReplyRepository;
pub use templates::{
    NotifyTemplateManagementRepository, NotifyTemplateRecord, PgNotifyTemplateRepository,
};

/// `LIKE` 的模式：以 `prefix` 开头。前缀里的 `\`、`%`、`_` 照字面匹配（配 `ESCAPE '\'`）。
fn like_prefix(prefix: &str) -> String {
    let mut pattern = String::with_capacity(prefix.len() + 1);

    for c in prefix.chars() {
        if matches!(c, '\\' | '%' | '_') {
            pattern.push('\\');
        }

        pattern.push(c);
    }

    pattern.push('%');
    pattern
}

#[cfg(test)]
mod tests {
    use super::like_prefix;

    #[test]
    fn a_prefix_matches_its_own_characters_literally() {
        assert_eq!(
            like_prefix("alert.escalated:a_b:"),
            "alert.escalated:a\\_b:%"
        );
        assert_eq!(like_prefix("100%\\"), "100\\%\\\\%");
    }
}
