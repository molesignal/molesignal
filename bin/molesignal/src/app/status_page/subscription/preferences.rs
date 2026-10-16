// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A subscriber changes what it hears about through the link every message
//! carries. The token in the link is the only proof needed: whoever can read
//! the subscriber's messages can manage the subscription.

use super::{super::StatusPageService, token_hash, validate_token};
use crate::{
    domain::status_page::{StatusPageLifecycle, StatusPageSubscriber},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

impl StatusPageService {
    /// The pending or active subscription the token belongs to.
    pub async fn subscription_preferences(
        &self,
        slug: &str,
        token: &str,
    ) -> Result<StatusPageSubscriber> {
        let page = self.get_customer_page(slug).await?;
        validate_token(token)?;

        self.repository
            .get_subscriber_by_token(&page.org_id, &page.id, &token_hash(token))
            .await
    }

    /// Changes the components the subscription hears about; none chosen means
    /// the whole page.
    pub async fn update_subscription_preferences(
        &self,
        slug: &str,
        token: &str,
        component_ids: Vec<Id>,
    ) -> Result<StatusPageSubscriber> {
        let page = self.get_customer_page(slug).await?;

        if page.lifecycle != StatusPageLifecycle::Active {
            return Err(Error::conflict(
                "subscriptions cannot be changed on an archived status page",
            ));
        }

        validate_token(token)?;
        let components = self.subscriber_components(&page, component_ids).await?;

        self.repository
            .update_subscriber_components(
                &page.org_id,
                &page.id,
                &token_hash(token),
                &components,
                TimestampMicros::now(),
            )
            .await
    }
}
