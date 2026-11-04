// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `/insight` 读的数：一个范围里的会话（本人的个人会话与这些团队的会话，不含自动化的）在
//! 一段时间里的问题、回答、评价、工具与 Skill，以及这些人与团队的自动化（加上组织的）的运行。

use std::collections::BTreeMap;

use serde_json::Value;
use sqlx::{PgPool, Row, types::Json};

use super::super::super::sqlx_err;
use crate::{
    agent::insights::{AutomationUse, InsightScope, Insights, ToolUse, ranked},
    shared::{Result, time::TimestampMicros},
};

/// 范围里的会话：$1 组织，$2 个人会话的主人（可空），$3 团队。
const SCOPE_CHATS: &str = "WITH scope_chats AS (
    SELECT id FROM agent_chats
    WHERE org_id = $1 AND deleted_at IS NULL AND automation_id IS NULL
      AND ((scope = 'personal' AND user_id = $2) OR team_id = ANY($3))
)";

pub(super) async fn insights(
    pool: &PgPool,
    scope: &InsightScope,
    since: TimestampMicros,
) -> Result<Insights> {
    let owner = scope.owner_id.as_ref().map(|id| id.0.clone());
    let teams = scope
        .team_ids
        .iter()
        .map(|id| id.0.clone())
        .collect::<Vec<_>>();

    // 每条语句都绑同样四个参数：组织、个人会话的主人、团队、起点。
    macro_rules! scoped {
        ($sql:expr) => {
            sqlx::query($sql)
                .bind(scope.org_id.0.clone())
                .bind(owner.clone())
                .bind(teams.clone())
                .bind(since)
        };
    }

    let sql = |body: &str| format!("{SCOPE_CHATS} {body}");

    let questions_sql = sql(
        "SELECT COUNT(DISTINCT m.chat_id) AS chats, COUNT(*) AS questions
         FROM agent_messages m JOIN scope_chats c ON c.id = m.chat_id
         WHERE m.role = 'user' AND m.created_at >= $4",
    );

    let questions = scoped!(&questions_sql)
        .fetch_one(pool)
        .await
        .map_err(sqlx_err)?;

    let runs_sql = sql("SELECT r.status, COUNT(*) AS count
         FROM agent_runs r JOIN scope_chats c ON c.id = r.chat_id
         WHERE r.created_at >= $4
         GROUP BY r.status");

    let runs = scoped!(&runs_sql)
        .fetch_all(pool)
        .await
        .map_err(sqlx_err)?
        .into_iter()
        .map(|row| Ok((row.try_get("status")?, row.try_get("count")?)))
        .collect::<std::result::Result<BTreeMap<String, i64>, sqlx::Error>>()
        .map_err(sqlx_err)?;

    let ratings_sql = sql("SELECT f.rating, f.reasons
         FROM agent_feedback f
         JOIN agent_messages m ON f.target_kind = 'chat_message' AND f.target_id = m.id
         JOIN scope_chats c ON c.id = m.chat_id
         WHERE f.updated_at >= $4");

    let ratings = scoped!(&ratings_sql)
        .fetch_all(pool)
        .await
        .map_err(sqlx_err)?;

    let (mut helpful, mut unhelpful, mut reasons) = (0, 0, Vec::new());

    for row in ratings {
        let rating: String = row.try_get("rating").map_err(sqlx_err)?;
        let given: Json<Value> = row.try_get("reasons").map_err(sqlx_err)?;

        if rating == "up" {
            helpful += 1;
        } else {
            unhelpful += 1;
            reasons.extend(
                given
                    .0
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(|reason| (reason.to_string(), 1)),
            );
        }
    }

    let tools_sql = sql("SELECT t.tool_name, COUNT(*) AS calls,
                COUNT(*) FILTER (WHERE t.status <> 'success') AS errors
         FROM agent_tool_calls t JOIN scope_chats c ON c.id = t.chat_id
         WHERE t.created_at >= $4
         GROUP BY t.tool_name ORDER BY calls DESC, t.tool_name LIMIT 20");

    let tools = scoped!(&tools_sql)
        .fetch_all(pool)
        .await
        .map_err(sqlx_err)?
        .into_iter()
        .map(|row| {
            Ok(ToolUse {
                name: row.try_get("tool_name")?,
                calls: row.try_get("calls")?,
                errors: row.try_get("errors")?,
            })
        })
        .collect::<std::result::Result<Vec<_>, sqlx::Error>>()
        .map_err(sqlx_err)?;

    // 用命令调的 Skill，加上模型自己读的（load_skill）。
    let skills_sql = sql("SELECT m.command_json->>'name' AS name, COUNT(*) AS count
         FROM agent_messages m JOIN scope_chats c ON c.id = m.chat_id
         WHERE m.role = 'user' AND m.created_at >= $4
           AND m.command_json->>'skillId' IS NOT NULL
         GROUP BY 1
         UNION ALL
         SELECT t.input->>'name' AS name, COUNT(*) AS count
         FROM agent_tool_calls t JOIN scope_chats c ON c.id = t.chat_id
         WHERE t.created_at >= $4 AND t.tool_name = 'load_skill'
         GROUP BY 1");

    let skills = scoped!(&skills_sql)
        .fetch_all(pool)
        .await
        .map_err(sqlx_err)?
        .into_iter()
        .map(|row| {
            let name: Option<String> = row.try_get("name")?;
            Ok((
                name.unwrap_or_default().trim_start_matches('/').to_string(),
                row.try_get("count")?,
            ))
        })
        .collect::<std::result::Result<Vec<(String, i64)>, sqlx::Error>>()
        .map_err(sqlx_err)?;

    let automations_sql = sql("SELECT a.name, r.status, COUNT(*) AS count
         FROM agent_automation_runs r JOIN agent_automations a ON a.id = r.automation_id
         WHERE r.org_id = $1 AND r.created_at >= $4
           AND (a.scope = 'org' OR (a.scope = 'personal' AND a.owner_id = $2)
                OR a.team_id = ANY($3))
         GROUP BY a.name, r.status ORDER BY a.name");

    let automations = scoped!(&automations_sql)
        .fetch_all(pool)
        .await
        .map_err(sqlx_err)?;

    let mut by_automation = BTreeMap::<String, BTreeMap<String, i64>>::new();

    for row in automations {
        let name: String = row.try_get("name").map_err(sqlx_err)?;
        let status: String = row.try_get("status").map_err(sqlx_err)?;
        let count: i64 = row.try_get("count").map_err(sqlx_err)?;
        by_automation.entry(name).or_default().insert(status, count);
    }

    Ok(Insights {
        since,
        chats: questions.try_get("chats").map_err(sqlx_err)?,
        questions: questions.try_get("questions").map_err(sqlx_err)?,
        runs,
        helpful,
        unhelpful,
        unhelpful_reasons: ranked(reasons),
        tools,
        skills: ranked(skills),
        automations: by_automation
            .into_iter()
            .map(|(name, runs)| AutomationUse { name, runs })
            .collect(),
    })
}
