// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 节假日日历：增删改查，并让本节点的缓存（[`holiday`] 里的那份）跟上。
//!
//! 每个节点启动时读一遍全部日历，之后每 [`REFRESH_EVERY`] 整份换一次；本节点改了日历立即换掉那
//! 一份，别的节点最多晚一个刷新周期。被引用着的日历不能删，免得引用它的时段悄悄换了算法。

use std::{sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use crate::{
    domain::alerting::holiday::{
        self, CalendarReference, HolidayCalendar, HolidayCalendarRepository, HolidayDay,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 多久整份刷新一次缓存。
const REFRESH_EVERY: Duration = Duration::from_secs(60);

/// 新建、修改一份日历时给的内容。
#[derive(Debug, Clone, Default)]
pub struct CalendarInput {
    pub name: String,
    pub description: String,
    pub days: Vec<HolidayDay>,
}

pub struct HolidayCalendars {
    repository: Arc<dyn HolidayCalendarRepository>,
}

impl HolidayCalendars {
    pub fn new(repository: Arc<dyn HolidayCalendarRepository>) -> Arc<Self> {
        Arc::new(Self { repository })
    }

    /// 读出全部日历换进缓存。
    pub async fn load(&self) -> Result<()> {
        let calendars = self.repository.list_all().await?;

        holiday::replace_all(&calendars);
        Ok(())
    }

    /// 定时刷新缓存；读不出时留着上一份。
    pub fn spawn_refresh(self: &Arc<Self>) -> JoinHandle<()> {
        let calendars = self.clone();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(REFRESH_EVERY);
            ticker.tick().await;

            loop {
                ticker.tick().await;

                if let Err(error) = calendars.load().await {
                    tracing::warn!(%error, "holiday calendars could not be refreshed");
                }
            }
        })
    }

    pub async fn list(&self, org_id: &Id) -> Result<Vec<HolidayCalendar>> {
        self.repository.list(org_id).await
    }

    pub async fn get(&self, org_id: &Id, id: &Id) -> Result<HolidayCalendar> {
        let calendar = self.repository.get(id).await?;

        if calendar.org_id != *org_id {
            return Err(Error::not_found("holiday calendar"));
        }

        Ok(calendar)
    }

    pub async fn create(
        &self,
        org_id: &Id,
        created_by: Option<Id>,
        input: CalendarInput,
    ) -> Result<HolidayCalendar> {
        let now = TimestampMicros::now();
        let mut calendar = HolidayCalendar {
            id: Id::new(),
            org_id: org_id.clone(),
            name: input.name,
            description: input.description,
            days: input.days,
            created_by,
            created_at: now,
            updated_at: now,
        };

        calendar.normalize()?;

        let created = self.repository.create(calendar).await?;

        holiday::remember(&created);
        Ok(created)
    }

    pub async fn update(
        &self,
        org_id: &Id,
        id: &Id,
        input: CalendarInput,
    ) -> Result<HolidayCalendar> {
        let mut calendar = self.get(org_id, id).await?;

        calendar.name = input.name;
        calendar.description = input.description;
        calendar.days = input.days;
        calendar.updated_at = TimestampMicros::now();
        calendar.normalize()?;

        let updated = self.repository.update(calendar).await?;

        holiday::remember(&updated);
        Ok(updated)
    }

    /// 删掉一份没人引用的日历，返回删掉的那份。
    pub async fn delete(&self, org_id: &Id, id: &Id) -> Result<HolidayCalendar> {
        let calendar = self.get(org_id, id).await?;
        let references = self.repository.references(org_id, id).await?;

        if let Some(first) = references.first() {
            return Err(Error::conflict(format!(
                "the holiday calendar is used by {} place(s), first the {} `{}`; take it out there first",
                references.len(),
                first.kind.replace('_', " "),
                first.name
            )));
        }

        self.repository.delete(id).await?;
        holiday::forget(id);
        Ok(calendar)
    }

    pub async fn references(&self, org_id: &Id, id: &Id) -> Result<Vec<CalendarReference>> {
        self.get(org_id, id).await?;
        self.repository.references(org_id, id).await
    }

    /// 联邦同步来的一份落了库：本节点的缓存跟上。
    pub async fn applied(&self, id: &Id) -> Result<()> {
        match self.repository.get(id).await {
            Ok(calendar) => holiday::remember(&calendar),
            Err(Error::NotFound(_)) => holiday::forget(id),
            Err(error) => return Err(error),
        }

        Ok(())
    }

    pub fn repository(&self) -> &Arc<dyn HolidayCalendarRepository> {
        &self.repository
    }
}
