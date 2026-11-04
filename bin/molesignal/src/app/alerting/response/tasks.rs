// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 重大事件的任务清单：加任务、改标题与负责人、勾选完成。加任务和做完任务记进时间线。

use serde_json::json;

use super::IncidentResponseService;
use crate::{
    domain::alerting::{
        incident::Incident,
        response::{IncidentTask, MAX_TASK_TITLE_CHARS, MAX_TASKS},
        timeline::TimelineKind,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

impl IncidentResponseService {
    pub async fn add_task(
        &self,
        incident: &Incident,
        title: &str,
        assignee_id: Option<Id>,
        by: &Id,
    ) -> Result<IncidentTask> {
        let title = task_title(title)?;
        let tasks = self
            .repository
            .list_tasks(&incident.org_id, &incident.id)
            .await?;

        if tasks.len() >= MAX_TASKS {
            return Err(Error::invalid(format!(
                "an incident has at most {MAX_TASKS} tasks"
            )));
        }

        let task = self
            .repository
            .save_task(IncidentTask {
                id: Id::new(),
                org_id: incident.org_id.clone(),
                incident_id: incident.id.clone(),
                title,
                assignee_id,
                done: false,
                done_by: None,
                done_at: None,
                position: tasks
                    .iter()
                    .map(|task| task.position)
                    .max()
                    .map_or(0, |last| last + 1),
                created_by: Some(by.clone()),
                created_at: TimestampMicros::now(),
            })
            .await?;

        self.record(
            incident,
            TimelineKind::TaskAdded,
            Some(by.clone()),
            json!({ "title": task.title }),
        )
        .await;

        Ok(task)
    }

    /// 改任务：标题、负责人、完成与否；`None` 的不改。
    pub async fn update_task(
        &self,
        incident: &Incident,
        task_id: &Id,
        title: Option<&str>,
        assignee_id: Option<Option<Id>>,
        done: Option<bool>,
        by: &Id,
    ) -> Result<IncidentTask> {
        let mut task = self
            .repository
            .list_tasks(&incident.org_id, &incident.id)
            .await?
            .into_iter()
            .find(|task| &task.id == task_id)
            .ok_or_else(|| Error::not_found("task not found"))?;

        if let Some(title) = title {
            task.title = task_title(title)?;
        }

        if let Some(assignee_id) = assignee_id {
            task.assignee_id = assignee_id;
        }

        let finished = done == Some(true) && !task.done;

        if let Some(done) = done {
            task.done = done;
            task.done_by = done.then(|| by.clone());
            task.done_at = done.then(TimestampMicros::now);
        }

        let task = self.repository.save_task(task).await?;

        if finished {
            self.record(
                incident,
                TimelineKind::TaskDone,
                Some(by.clone()),
                json!({ "title": task.title }),
            )
            .await;
        }

        Ok(task)
    }
}

fn task_title(title: &str) -> Result<String> {
    let title = title.trim();

    if title.is_empty() || title.chars().count() > MAX_TASK_TITLE_CHARS {
        return Err(Error::invalid(format!(
            "a task has 1 to {MAX_TASK_TITLE_CHARS} characters"
        )));
    }

    Ok(title.to_string())
}
