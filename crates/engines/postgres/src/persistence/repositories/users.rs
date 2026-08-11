// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use super::sqlx_err;
use crate::{
    domain::iam::{User, UserRepository, UserStatus},
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct PgUserRepository {
    pool: PgPool,
}

impl PgUserRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to_user(row: sqlx::postgres::PgRow) -> Result<User> {
    Ok(User {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        email: row.try_get("email").map_err(sqlx_err)?,
        display_name: row.try_get("display_name").map_err(sqlx_err)?,
        avatar_url: row.try_get("avatar_url").map_err(sqlx_err)?,
        bio: row.try_get("bio").map_err(sqlx_err)?,
        password_hash: row.try_get("password_hash").map_err(sqlx_err)?,
        disabled: row.try_get("disabled").map_err(sqlx_err)?,
        status: match row
            .try_get::<String, _>("status")
            .map_err(sqlx_err)?
            .as_str()
        {
            "pending" => UserStatus::Pending,
            "rejected" => UserStatus::Rejected,
            _ => UserStatus::Active,
        },
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
    })
}

const COLS: &str =
    "id, email, display_name, avatar_url, bio, password_hash, disabled, status, created_at_micros";

#[async_trait]
impl UserRepository for PgUserRepository {
    async fn create(&self, user: User) -> Result<User> {
        sqlx::query(
            "INSERT INTO users (id, email, display_name, avatar_url, bio, password_hash, disabled, status, created_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(&user.id.0)
        .bind(&user.email)
        .bind(&user.display_name)
        .bind(&user.avatar_url)
        .bind(&user.bio)
        .bind(&user.password_hash)
        .bind(user.disabled)
        .bind(match user.status {
            UserStatus::Active => "active",
            UserStatus::Pending => "pending",
            UserStatus::Rejected => "rejected",
        })
        .bind(user.created_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(user)
    }

    async fn get(&self, id: &Id) -> Result<User> {
        let row = sqlx::query(&format!("SELECT {COLS} FROM users WHERE id = $1"))
            .bind(&id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;
        row_to_user(row)
    }

    async fn get_by_email(&self, email: &str) -> Result<User> {
        let row = sqlx::query(&format!("SELECT {COLS} FROM users WHERE email = $1"))
            .bind(email)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;
        row_to_user(row)
    }

    async fn update(&self, user: User) -> Result<User> {
        sqlx::query(
            "UPDATE users SET email = $2, display_name = $3, avatar_url = $4, bio = $5, password_hash = $6, disabled = $7
             WHERE id = $1",
        )
        .bind(&user.id.0)
        .bind(&user.email)
        .bind(&user.display_name)
        .bind(&user.avatar_url)
        .bind(&user.bio)
        .bind(&user.password_hash)
        .bind(user.disabled)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(user)
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(())
    }

    async fn count(&self) -> Result<u64> {
        let row = sqlx::query("SELECT COUNT(*)::BIGINT AS n FROM users")
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;
        let n: i64 = row.try_get("n").map_err(sqlx_err)?;
        Ok(n.max(0) as u64)
    }

    async fn list(&self) -> Result<Vec<User>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM users ORDER BY created_at_micros"
        ))
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(row_to_user).collect()
    }

    async fn set_status(&self, id: &Id, status: UserStatus) -> Result<()> {
        let s = match status {
            UserStatus::Active => "active",
            UserStatus::Pending => "pending",
            UserStatus::Rejected => "rejected",
        };
        sqlx::query("UPDATE users SET status = $2 WHERE id = $1")
            .bind(&id.0)
            .bind(s)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(())
    }
}
