// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::Value;
use sqlx::{PgPool, Row, types::Json};

use super::super::super::sqlx_err;
use crate::{
    agent::inbound_mcp::{
        InboundMcpAuthorizationCode, InboundMcpOAuthClient, InboundMcpOAuthConnection,
        InboundMcpOAuthToken, InboundMcpOAuthTokenKind, NewInboundMcpOAuthClient,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const CLIENT_COLS: &str = "client_id,client_name,redirect_uris,grant_types,response_types,\
    token_endpoint_auth_method,scope,client_secret_hash,client_id_issued_at,\
    client_secret_expires_at,client_uri,software_id,software_version,\
    created_at_micros,updated_at_micros";
const TOKEN_COLS: &str = "id,token_hash,token_kind,family_id,client_id,org_id,user_id,scope,\
    resource,expires_at_micros,revoked_at_micros,rotated_from,created_at_micros";

fn json_strings(row: &sqlx::postgres::PgRow, column: &str) -> Result<Vec<String>> {
    let value: Json<Value> = row.try_get(column).map_err(sqlx_err)?;
    serde_json::from_value(value.0)
        .map_err(|error| Error::internal(format!("invalid OAuth `{column}`: {error}")))
}

fn client_row(row: sqlx::postgres::PgRow) -> Result<InboundMcpOAuthClient> {
    Ok(InboundMcpOAuthClient {
        client_id: row.try_get("client_id").map_err(sqlx_err)?,
        client_name: row.try_get("client_name").map_err(sqlx_err)?,
        redirect_uris: json_strings(&row, "redirect_uris")?,
        grant_types: json_strings(&row, "grant_types")?,
        response_types: json_strings(&row, "response_types")?,
        token_endpoint_auth_method: row
            .try_get("token_endpoint_auth_method")
            .map_err(sqlx_err)?,
        scope: row.try_get("scope").map_err(sqlx_err)?,
        client_secret_hash: row.try_get("client_secret_hash").map_err(sqlx_err)?,
        client_id_issued_at: row.try_get("client_id_issued_at").map_err(sqlx_err)?,
        client_secret_expires_at: row.try_get("client_secret_expires_at").map_err(sqlx_err)?,
        client_uri: row.try_get("client_uri").map_err(sqlx_err)?,
        software_id: row.try_get("software_id").map_err(sqlx_err)?,
        software_version: row.try_get("software_version").map_err(sqlx_err)?,
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

pub(super) async fn create_client(
    pool: &PgPool,
    client: NewInboundMcpOAuthClient,
) -> Result<InboundMcpOAuthClient> {
    let now = TimestampMicros::now();
    let row = sqlx::query(&format!(
        "INSERT INTO inbound_mcp_oauth_clients ({CLIENT_COLS})
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$14)
         RETURNING {CLIENT_COLS}"
    ))
    .bind(client.client_id)
    .bind(client.client_name)
    .bind(Json(client.redirect_uris))
    .bind(Json(client.grant_types))
    .bind(Json(client.response_types))
    .bind(client.token_endpoint_auth_method)
    .bind(client.scope)
    .bind(client.client_secret_hash)
    .bind(client.client_id_issued_at)
    .bind(client.client_secret_expires_at)
    .bind(client.client_uri)
    .bind(client.software_id)
    .bind(client.software_version)
    .bind(now.0)
    .fetch_one(pool)
    .await
    .map_err(sqlx_err)?;
    client_row(row)
}

pub(super) async fn get_client(
    pool: &PgPool,
    client_id: &str,
) -> Result<Option<InboundMcpOAuthClient>> {
    sqlx::query(&format!(
        "SELECT {CLIENT_COLS} FROM inbound_mcp_oauth_clients WHERE client_id=$1"
    ))
    .bind(client_id)
    .fetch_optional(pool)
    .await
    .map_err(sqlx_err)?
    .map(client_row)
    .transpose()
}

pub(super) async fn list_clients(pool: &PgPool) -> Result<Vec<InboundMcpOAuthClient>> {
    sqlx::query(&format!(
        "SELECT {CLIENT_COLS} FROM inbound_mcp_oauth_clients
         ORDER BY created_at_micros DESC,client_id ASC LIMIT 1000"
    ))
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?
    .into_iter()
    .map(client_row)
    .collect()
}

pub(super) async fn delete_client(pool: &PgPool, client_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM inbound_mcp_oauth_clients WHERE client_id=$1")
        .bind(client_id)
        .execute(pool)
        .await
        .map_err(sqlx_err)?;
    Ok(())
}

pub(super) async fn create_code(pool: &PgPool, code: InboundMcpAuthorizationCode) -> Result<()> {
    sqlx::query(
        "INSERT INTO inbound_mcp_oauth_codes
            (code_hash,client_id,org_id,user_id,redirect_uri,scope,resource,code_challenge,
             code_challenge_method,expires_at_micros,consumed_at_micros,created_at_micros)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,NULL,$11)",
    )
    .bind(code.code_hash)
    .bind(code.client_id)
    .bind(code.org_id.0)
    .bind(code.user_id.0)
    .bind(code.redirect_uri)
    .bind(code.scope)
    .bind(code.resource)
    .bind(code.code_challenge)
    .bind(code.code_challenge_method)
    .bind(code.expires_at.0)
    .bind(code.created_at.0)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;
    Ok(())
}

fn code_row(row: sqlx::postgres::PgRow) -> Result<InboundMcpAuthorizationCode> {
    Ok(InboundMcpAuthorizationCode {
        code_hash: row.try_get("code_hash").map_err(sqlx_err)?,
        client_id: row.try_get("client_id").map_err(sqlx_err)?,
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        user_id: Id(row.try_get("user_id").map_err(sqlx_err)?),
        redirect_uri: row.try_get("redirect_uri").map_err(sqlx_err)?,
        scope: row.try_get("scope").map_err(sqlx_err)?,
        resource: row.try_get("resource").map_err(sqlx_err)?,
        code_challenge: row.try_get("code_challenge").map_err(sqlx_err)?,
        code_challenge_method: row.try_get("code_challenge_method").map_err(sqlx_err)?,
        expires_at: TimestampMicros(row.try_get("expires_at_micros").map_err(sqlx_err)?),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
    })
}

pub(super) async fn consume_code(
    pool: &PgPool,
    code_hash: &str,
    client_id: &str,
    redirect_uri: &str,
    resource: &str,
    code_challenge: &str,
    now: TimestampMicros,
) -> Result<Option<InboundMcpAuthorizationCode>> {
    sqlx::query(
        "UPDATE inbound_mcp_oauth_codes SET consumed_at_micros=$6
         WHERE code_hash=$1 AND client_id=$2 AND redirect_uri=$3 AND resource=$4
           AND code_challenge=$5 AND code_challenge_method='S256'
           AND consumed_at_micros IS NULL AND expires_at_micros>$6
         RETURNING code_hash,client_id,org_id,user_id,redirect_uri,scope,resource,
                   code_challenge,code_challenge_method,expires_at_micros,created_at_micros",
    )
    .bind(code_hash)
    .bind(client_id)
    .bind(redirect_uri)
    .bind(resource)
    .bind(code_challenge)
    .bind(now.0)
    .fetch_optional(pool)
    .await
    .map_err(sqlx_err)?
    .map(code_row)
    .transpose()
}

fn token_kind(value: &str) -> Result<InboundMcpOAuthTokenKind> {
    match value {
        "access" => Ok(InboundMcpOAuthTokenKind::Access),
        "refresh" => Ok(InboundMcpOAuthTokenKind::Refresh),
        _ => Err(Error::internal(format!(
            "invalid OAuth token kind `{value}`"
        ))),
    }
}

fn token_row(row: sqlx::postgres::PgRow) -> Result<InboundMcpOAuthToken> {
    Ok(InboundMcpOAuthToken {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        token_hash: row.try_get("token_hash").map_err(sqlx_err)?,
        token_kind: token_kind(&row.try_get::<String, _>("token_kind").map_err(sqlx_err)?)?,
        family_id: Id(row.try_get("family_id").map_err(sqlx_err)?),
        client_id: row.try_get("client_id").map_err(sqlx_err)?,
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        user_id: Id(row.try_get("user_id").map_err(sqlx_err)?),
        scope: row.try_get("scope").map_err(sqlx_err)?,
        resource: row.try_get("resource").map_err(sqlx_err)?,
        expires_at: TimestampMicros(row.try_get("expires_at_micros").map_err(sqlx_err)?),
        revoked_at: row
            .try_get::<Option<i64>, _>("revoked_at_micros")
            .map_err(sqlx_err)?
            .map(TimestampMicros),
        rotated_from: row
            .try_get::<Option<String>, _>("rotated_from")
            .map_err(sqlx_err)?
            .map(Id),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
    })
}

async fn insert_token(
    connection: &mut sqlx::PgConnection,
    token: InboundMcpOAuthToken,
) -> Result<()> {
    let inserted = sqlx::query(&format!(
        "INSERT INTO inbound_mcp_oauth_tokens ({TOKEN_COLS})
         SELECT $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13
         WHERE $5 LIKE 'https://%' OR EXISTS (
             SELECT 1 FROM inbound_mcp_oauth_clients WHERE client_id=$5
         )"
    ))
    .bind(token.id.0)
    .bind(token.token_hash)
    .bind(token.token_kind.as_str())
    .bind(token.family_id.0)
    .bind(token.client_id)
    .bind(token.org_id.0)
    .bind(token.user_id.0)
    .bind(token.scope)
    .bind(token.resource)
    .bind(token.expires_at.0)
    .bind(token.revoked_at.map(|value| value.0))
    .bind(token.rotated_from.map(|value| value.0))
    .bind(token.created_at.0)
    .execute(connection)
    .await
    .map_err(sqlx_err)?
    .rows_affected();
    if inserted != 1 {
        return Err(Error::unauthorized(
            "OAuth client was removed before token issuance completed",
        ));
    }
    Ok(())
}

pub(super) async fn create_token_pair(
    pool: &PgPool,
    access: InboundMcpOAuthToken,
    refresh: Option<InboundMcpOAuthToken>,
) -> Result<()> {
    let mut transaction = sqlx::begin(pool).await.map_err(sqlx_err)?;
    insert_token(&mut transaction, access).await?;
    if let Some(refresh) = refresh {
        insert_token(&mut transaction, refresh).await?;
    }
    transaction.commit().await.map_err(sqlx_err)?;
    Ok(())
}

pub(super) async fn rotate_token_pair(
    pool: &PgPool,
    refresh_token_hash: &str,
    now: TimestampMicros,
    access: InboundMcpOAuthToken,
    refresh: Option<InboundMcpOAuthToken>,
) -> Result<bool> {
    let mut transaction = sqlx::begin(pool).await.map_err(sqlx_err)?;
    let consumed = sqlx::query(
        "UPDATE inbound_mcp_oauth_tokens SET revoked_at_micros=$2
         WHERE token_hash=$1 AND token_kind='refresh' AND revoked_at_micros IS NULL
           AND expires_at_micros>$2",
    )
    .bind(refresh_token_hash)
    .bind(now.0)
    .execute(&mut *transaction)
    .await
    .map_err(sqlx_err)?
    .rows_affected();
    if consumed != 1 {
        transaction.rollback().await.map_err(sqlx_err)?;
        return Ok(false);
    }
    insert_token(&mut transaction, access).await?;
    if let Some(refresh) = refresh {
        insert_token(&mut transaction, refresh).await?;
    }
    transaction.commit().await.map_err(sqlx_err)?;
    Ok(true)
}

pub(super) async fn find_token(
    pool: &PgPool,
    token_hash: &str,
) -> Result<Option<InboundMcpOAuthToken>> {
    sqlx::query(&format!(
        "SELECT {TOKEN_COLS} FROM inbound_mcp_oauth_tokens WHERE token_hash=$1"
    ))
    .bind(token_hash)
    .fetch_optional(pool)
    .await
    .map_err(sqlx_err)?
    .map(token_row)
    .transpose()
}

pub(super) async fn list_tokens(pool: &PgPool, org_id: &Id) -> Result<Vec<InboundMcpOAuthToken>> {
    sqlx::query(&format!(
        "SELECT {TOKEN_COLS} FROM inbound_mcp_oauth_tokens
         WHERE org_id=$1 ORDER BY created_at_micros DESC LIMIT 500"
    ))
    .bind(&org_id.0)
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?
    .into_iter()
    .map(token_row)
    .collect()
}

pub(super) async fn list_connections(
    pool: &PgPool,
    org_id: &Id,
    now: TimestampMicros,
) -> Result<Vec<InboundMcpOAuthConnection>> {
    let rows = sqlx::query(
        "WITH family_summary AS (
            SELECT family_id,MIN(created_at_micros) AS created_at_micros,
                   MAX(expires_at_micros) AS last_expires_at_micros,
                   BOOL_OR(revoked_at_micros IS NULL AND expires_at_micros>$2) AS active
              FROM inbound_mcp_oauth_tokens
             WHERE org_id=$1
             GROUP BY family_id
         ), latest AS (
            SELECT DISTINCT ON (family_id)
                   family_id,client_id,user_id,scope,resource
              FROM inbound_mcp_oauth_tokens
             WHERE org_id=$1
             ORDER BY family_id,created_at_micros DESC,id DESC
         )
         SELECT summary.family_id,latest.client_id,
                COALESCE(client.client_name,latest.client_id) AS client_name,
                latest.user_id,account.display_name AS user_display_name,
                latest.scope,latest.resource,
                summary.created_at_micros,summary.last_expires_at_micros,summary.active
           FROM family_summary summary
           JOIN latest USING (family_id)
           LEFT JOIN inbound_mcp_oauth_clients client ON client.client_id=latest.client_id
           JOIN users account ON account.id=latest.user_id
          ORDER BY summary.created_at_micros DESC,summary.family_id ASC
          LIMIT 1000",
    )
    .bind(&org_id.0)
    .bind(now.0)
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?;
    rows.into_iter()
        .map(|row| {
            Ok(InboundMcpOAuthConnection {
                family_id: Id(row.try_get("family_id").map_err(sqlx_err)?),
                client_id: row.try_get("client_id").map_err(sqlx_err)?,
                client_name: row.try_get("client_name").map_err(sqlx_err)?,
                user_id: Id(row.try_get("user_id").map_err(sqlx_err)?),
                user_display_name: row.try_get("user_display_name").map_err(sqlx_err)?,
                scope: row.try_get("scope").map_err(sqlx_err)?,
                resource: row.try_get("resource").map_err(sqlx_err)?,
                created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
                last_expires_at: TimestampMicros(
                    row.try_get("last_expires_at_micros").map_err(sqlx_err)?,
                ),
                active: row.try_get("active").map_err(sqlx_err)?,
            })
        })
        .collect()
}

pub(super) async fn revoke_token(
    pool: &PgPool,
    token_hash: &str,
    now: TimestampMicros,
) -> Result<()> {
    sqlx::query(
        "UPDATE inbound_mcp_oauth_tokens SET revoked_at_micros=COALESCE(revoked_at_micros,$2)
         WHERE token_hash=$1",
    )
    .bind(token_hash)
    .bind(now.0)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;
    Ok(())
}

pub(super) async fn revoke_family(
    pool: &PgPool,
    org_id: &Id,
    family_id: &Id,
    now: TimestampMicros,
) -> Result<()> {
    sqlx::query(
        "UPDATE inbound_mcp_oauth_tokens SET revoked_at_micros=COALESCE(revoked_at_micros,$2)
         WHERE family_id=$1 AND org_id=$3",
    )
    .bind(&family_id.0)
    .bind(now.0)
    .bind(&org_id.0)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;
    Ok(())
}

pub(super) async fn revoke_client_tokens(
    pool: &PgPool,
    org_id: &Id,
    client_id: &str,
    now: TimestampMicros,
) -> Result<()> {
    sqlx::query(
        "UPDATE inbound_mcp_oauth_tokens SET revoked_at_micros=COALESCE(revoked_at_micros,$2)
         WHERE client_id=$1 AND org_id=$3",
    )
    .bind(client_id)
    .bind(now.0)
    .bind(&org_id.0)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;
    Ok(())
}
