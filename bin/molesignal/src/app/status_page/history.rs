// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::StatusPageService;
use crate::{
    domain::status_page::{StatusPageHistoryPage, StatusPageHistoryQuery},
    shared::{Error, Result, ids::Id},
};

impl StatusPageService {
    pub async fn query_history(
        &self,
        org_id: &Id,
        page_id: &Id,
        mut query: StatusPageHistoryQuery,
    ) -> Result<StatusPageHistoryPage> {
        self.repository.get_page(org_id, page_id).await?;
        query.page = query.page.max(1);
        query.per_page = 25;
        query.search = query
            .search
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        if query
            .search
            .as_ref()
            .is_some_and(|value| value.chars().count() > 200)
        {
            return Err(Error::invalid(
                "history search cannot exceed 200 characters",
            ));
        }
        if query.from.zip(query.to).is_some_and(|(from, to)| from > to) {
            return Err(Error::invalid("history date range is invalid"));
        }
        self.repository
            .query_status_history(org_id, page_id, &query)
            .await
    }
}
