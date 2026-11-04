// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 组织级设置的存取：`org_settings` 里一个组织每一节一行，`value` 是这一节的 JSON。各节的含义、默认值
//! 与校验在各自的仓库和领域类型里，这里只管读写。

use serde::{Serialize, de::DeserializeOwned};
use sqlx::{PgConnection, PgPool, Row, postgres::PgRow, types::Json};

use super::sqlx_err;
use crate::shared::{Result, time::TimestampMicros};

/// 各节的名字。
pub mod sections {
    /// 会话结束后是否由模型提炼记忆。
    pub const AGENT_MEMORY: &str = "agent.memory";
    /// 团队成员与个人建的连接（Skill 仓库、MCP 服务器）可以连的内网主机与网段。
    pub const AGENT_NETWORK: &str = "agent.network";
    /// 回答评分：进入待审的分数线与抽样比例，设默认 Prompt 前的检查。
    pub const AGENT_SCORING: &str = "agent.scoring";
    /// 各风险等级默认的执行方式与按环境的覆盖。
    pub const AGENT_TOOL_POLICY: &str = "agent.tool_policy";
    /// 每日 token 预算。
    pub const AGENT_USAGE_BUDGET: &str = "agent.usage_budget";
    /// 模型服务单价可选的币种。
    pub const AGENT_CURRENCIES: &str = "agent.currencies";
    /// 实例级 Skill 的启停。
    pub const AGENT_SKILLS: &str = "agent.skills";
    /// APM：健康线、默认 Apdex T、命名规则、环境映射等。
    pub const APM: &str = "apm";
    /// 对外的 MCP Server。
    pub const INBOUND_MCP: &str = "inbound_mcp";
    /// 优先级与紧急度档位。
    pub const INCIDENT_IMPORTANCE: &str = "incident.importance";
    /// 重大事件：建群、会议、检查清单、自动宣布；会议密钥放在 `secret_*`。
    pub const INCIDENT_RESPONSE: &str = "incident.response";
    /// 登录要求：多因素认证、只允许单点登录。
    pub const SIGNIN: &str = "signin";
    /// 资源分享：公开链接、公开仪表盘、有效期等。
    pub const RESOURCE_SHARING: &str = "resource_sharing";
    /// 成员没有自己设置时用的界面偏好。
    pub const WORKSPACE_PREFERENCES: &str = "workspace_preferences";
}

/// 读出来的一节。
pub struct Section<T> {
    pub value: T,
    pub updated_by: Option<String>,
    pub updated_at: TimestampMicros,
}

fn section_row<T: DeserializeOwned>(row: &PgRow) -> Result<Section<T>> {
    Ok(Section {
        value: row.try_get::<Json<T>, _>("value").map_err(sqlx_err)?.0,
        updated_by: row.try_get("updated_by").map_err(sqlx_err)?,
        updated_at: row.try_get("updated_at").map_err(sqlx_err)?,
    })
}

/// 读一个组织的一节；还没写过时是 `None`。
pub async fn read<T: DeserializeOwned>(
    pool: &PgPool,
    org_id: &str,
    section: &str,
) -> Result<Option<Section<T>>> {
    sqlx::query(
        "SELECT value, updated_by, updated_at FROM org_settings
         WHERE org_id = $1 AND section = $2",
    )
    .bind(org_id)
    .bind(section)
    .fetch_optional(pool)
    .await
    .map_err(sqlx_err)?
    .map(|row| section_row(&row))
    .transpose()
}

/// 读所有写过这一节的组织，按组织 id 排。
pub async fn read_all<T: DeserializeOwned>(
    pool: &PgPool,
    section: &str,
) -> Result<Vec<(String, Section<T>)>> {
    sqlx::query(
        "SELECT org_id, value, updated_by, updated_at FROM org_settings
         WHERE section = $1 ORDER BY org_id",
    )
    .bind(section)
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?
    .iter()
    .map(|row| Ok((row.try_get("org_id").map_err(sqlx_err)?, section_row(row)?)))
    .collect()
}

/// 在事务里读一节并锁住这一行，改完用 [`write_in`] 写回。还没有这一行时不加锁。
pub async fn read_for_update<T: DeserializeOwned>(
    connection: &mut PgConnection,
    org_id: &str,
    section: &str,
) -> Result<Option<Section<T>>> {
    sqlx::query(
        "SELECT value, updated_by, updated_at FROM org_settings
         WHERE org_id = $1 AND section = $2 FOR UPDATE",
    )
    .bind(org_id)
    .bind(section)
    .fetch_optional(connection)
    .await
    .map_err(sqlx_err)?
    .map(|row| section_row(&row))
    .transpose()
}

/// 整节换掉。
pub async fn write<T: Serialize + Sync>(
    pool: &PgPool,
    org_id: &str,
    section: &str,
    value: &T,
    updated_by: Option<&str>,
    updated_at: TimestampMicros,
) -> Result<()> {
    let mut connection = pool.acquire().await.map_err(sqlx_err)?;
    write_in(
        &mut connection,
        org_id,
        section,
        value,
        updated_by,
        updated_at,
    )
    .await
}

/// 在给定的连接或事务里整节换掉。
pub async fn write_in<T: Serialize + Sync>(
    connection: &mut PgConnection,
    org_id: &str,
    section: &str,
    value: &T,
    updated_by: Option<&str>,
    updated_at: TimestampMicros,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO org_settings (org_id, section, value, updated_by, updated_at)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (org_id, section) DO UPDATE SET
            value = EXCLUDED.value,
            updated_by = EXCLUDED.updated_by,
            updated_at = EXCLUDED.updated_at",
    )
    .bind(org_id)
    .bind(section)
    .bind(Json(value))
    .bind(updated_by)
    .bind(updated_at)
    .execute(connection)
    .await
    .map_err(sqlx_err)?;

    Ok(())
}

/// 删掉一节，回到默认值；返回原来有没有。
pub async fn delete(pool: &PgPool, org_id: &str, section: &str) -> Result<bool> {
    let deleted = sqlx::query("DELETE FROM org_settings WHERE org_id = $1 AND section = $2")
        .bind(org_id)
        .bind(section)
        .execute(pool)
        .await
        .map_err(sqlx_err)?
        .rows_affected();

    Ok(deleted > 0)
}
