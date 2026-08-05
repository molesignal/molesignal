// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! KEK（根密钥）轮换：离线重包所有被 `CipherRootKey` 信封包过的列。
//!
//! 换 `MS_CIPHER_KEY` 时，DB 里所有用旧 KEK seal 的 `(nonce, ciphertext)` 必须用旧 KEK 解、
//! 新 KEK 重封——否则换 key 后这些数据不可解。本模块用一份**显式表清单** [`SPECS`] 覆盖全部
//! KEK-sealed 列（cipher_keys / cluster_secrets / billing_settings / api_tokens /
//! resource_shares / ai_model_provider_secrets / notify_connectors），逐行
//! open(old) → seal(new) → UPDATE，
//! 返回每表重包行数供核对。
//!
//! **离线运维操作**：应在停写窗口执行；新 KEK 已就位（`MS_CIPHER_KEY`）、旧 KEK 由调用方提供；
//! 跑完核对各表计数后方可下线旧 KEK。字段数据本身不动（只换信封）。

use sqlx::{PgPool, Row};

use super::keys::root::CipherRootKey;
use crate::infra::persistence::sqlx_err;

/// 一张表里一组 KEK-sealed 列的重包规格。
struct RewrapSpec {
    table: &'static str,
    /// 主键列（定位行做 UPDATE）；本项目这些表 PK 均为 VARCHAR。
    pk: &'static [&'static str],
    /// `(nonce_col, ciphertext_col)` 对；可空对在行内缺一即跳过（如未配的 secret）。
    pairs: &'static [(&'static str, &'static str)],
}

/// 全部 KEK-sealed 列清单（与各 repo 的 seal/open 落库一一对应）。
const SPECS: &[RewrapSpec] = &[
    RewrapSpec {
        table: "cipher_keys",
        pk: &["id"],
        pairs: &[("nonce", "key_material_enc")],
    },
    RewrapSpec {
        table: "cluster_secrets",
        pk: &["org_id", "ref_id"],
        pairs: &[("nonce", "ciphertext")],
    },
    RewrapSpec {
        table: "billing_settings",
        pk: &["id"],
        pairs: &[
            ("webhook_secret_nonce", "webhook_secret_ciphertext"),
            ("api_key_nonce", "api_key_ciphertext"),
        ],
    },
    RewrapSpec {
        table: "api_tokens",
        pk: &["id"],
        pairs: &[("plaintext_nonce", "plaintext_sealed")],
    },
    RewrapSpec {
        table: "resource_shares",
        pk: &["id"],
        pairs: &[("token_nonce", "token_ciphertext")],
    },
    RewrapSpec {
        table: "ai_model_provider_secrets",
        pk: &["provider_id"],
        pairs: &[("nonce", "ciphertext")],
    },
    RewrapSpec {
        table: "notify_connectors",
        pk: &["id"],
        pairs: &[("config_nonce", "config_ciphertext")],
    },
];

/// 重包单个 `(nonce, ciphertext)`：旧 KEK 解、新 KEK 封。返回新的 `(nonce, ciphertext)`。
fn rewrap_blob(
    old: &CipherRootKey,
    new: &CipherRootKey,
    nonce: &[u8],
    ct: &[u8],
) -> crate::shared::Result<(Vec<u8>, Vec<u8>)> {
    use crate::shared::Error;
    let raw = old
        .open(nonce, ct)
        .map_err(|e| Error::invalid(format!("rewrap open with old KEK: {e}")))?;
    new.seal(&raw)
        .map_err(|e| Error::internal(format!("rewrap seal with new KEK: {e}")))
}

fn select_sql(spec: &RewrapSpec) -> String {
    let mut cols: Vec<&str> = spec.pk.to_vec();
    for (n, c) in spec.pairs {
        cols.push(n);
        cols.push(c);
    }
    format!("SELECT {} FROM {}", cols.join(", "), spec.table)
}

fn update_sql(table: &str, set_cols: &[&str], pk: &[&str]) -> String {
    let mut idx = 1;
    let sets: Vec<String> = set_cols
        .iter()
        .map(|c| {
            let s = format!("{c} = ${idx}");
            idx += 1;
            s
        })
        .collect();
    let wheres: Vec<String> = pk
        .iter()
        .map(|c| {
            let s = format!("{c} = ${idx}");
            idx += 1;
            s
        })
        .collect();
    format!(
        "UPDATE {} SET {} WHERE {}",
        table,
        sets.join(", "),
        wheres.join(" AND ")
    )
}

async fn rewrap_table(
    pool: &PgPool,
    old: &CipherRootKey,
    new: &CipherRootKey,
    spec: &RewrapSpec,
) -> crate::shared::Result<usize> {
    let rows = sqlx::query(&select_sql(spec))
        .fetch_all(pool)
        .await
        .map_err(sqlx_err)?;
    let mut updated = 0usize;
    for row in &rows {
        // 重包本行所有非空 pair。
        let mut set_cols: Vec<&str> = Vec::new();
        let mut set_vals: Vec<Vec<u8>> = Vec::new();
        for (ncol, ccol) in spec.pairs {
            let nonce: Option<Vec<u8>> = row.try_get(*ncol).map_err(sqlx_err)?;
            let ct: Option<Vec<u8>> = row.try_get(*ccol).map_err(sqlx_err)?;
            if let (Some(n), Some(c)) = (nonce, ct) {
                let (nn, nc) = rewrap_blob(old, new, &n, &c)?;
                set_cols.push(ncol);
                set_vals.push(nn);
                set_cols.push(ccol);
                set_vals.push(nc);
            }
        }
        if set_cols.is_empty() {
            continue; // 该行无（已配的）密文列
        }
        let pk_vals: Vec<String> = spec
            .pk
            .iter()
            .map(|c| row.try_get::<String, _>(*c).map_err(sqlx_err))
            .collect::<crate::shared::Result<_>>()?;
        let sql = update_sql(spec.table, &set_cols, spec.pk);
        let mut q = sqlx::query(&sql);
        for v in &set_vals {
            q = q.bind(v.as_slice());
        }
        for pk in &pk_vals {
            q = q.bind(pk.as_str());
        }
        q.execute(pool).await.map_err(sqlx_err)?;
        updated += 1;
    }
    Ok(updated)
}

/// 重包全部 KEK-sealed 列，返回 `(table, rewrapped_rows)` 列表供运维核对。
pub async fn rewrap_all(
    pool: &PgPool,
    old: &CipherRootKey,
    new: &CipherRootKey,
) -> crate::shared::Result<Vec<(String, usize)>> {
    let mut report = Vec::with_capacity(SPECS.len());
    for spec in SPECS {
        let n = rewrap_table(pool, old, new, spec).await?;
        report.push((spec.table.to_string(), n));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;

    use super::*;

    fn kek(byte: u8) -> CipherRootKey {
        let b64 = base64::engine::general_purpose::STANDARD.encode([byte; 32]);
        CipherRootKey::from_base64(&b64).unwrap()
    }

    #[test]
    fn rewrap_blob_round_trips_old_to_new() {
        let old = kek(1);
        let new = kek(2);
        let (nonce, ct) = old.seal(b"super-secret").unwrap();
        let (nn, nc) = rewrap_blob(&old, &new, &nonce, &ct).unwrap();
        // 新信封用新 KEK 解得回原文；旧 KEK 不再能解新信封。
        assert_eq!(new.open(&nn, &nc).unwrap(), b"super-secret");
        assert!(old.open(&nn, &nc).is_err());
    }

    #[test]
    fn rewrap_blob_rejects_wrong_old_key() {
        let real_old = kek(1);
        let wrong_old = kek(9);
        let new = kek(2);
        let (nonce, ct) = real_old.seal(b"x").unwrap();
        assert!(rewrap_blob(&wrong_old, &new, &nonce, &ct).is_err());
    }

    #[test]
    fn select_sql_lists_pk_then_pairs() {
        let spec = &SPECS[2]; // billing_settings：id + 两组 pair
        assert_eq!(
            select_sql(spec),
            "SELECT id, webhook_secret_nonce, webhook_secret_ciphertext, \
             api_key_nonce, api_key_ciphertext FROM billing_settings"
        );
    }

    #[test]
    fn update_sql_numbers_set_then_where() {
        let sql = update_sql(
            "cluster_secrets",
            &["nonce", "ciphertext"],
            &["org_id", "ref_id"],
        );
        assert_eq!(
            sql,
            "UPDATE cluster_secrets SET nonce = $1, ciphertext = $2 WHERE org_id = $3 AND ref_id = $4"
        );
    }

    #[test]
    fn specs_cover_all_known_sealed_tables() {
        let tables: Vec<&str> = SPECS.iter().map(|s| s.table).collect();
        for t in [
            "cipher_keys",
            "cluster_secrets",
            "billing_settings",
            "api_tokens",
            "resource_shares",
            "ai_model_provider_secrets",
            "notify_connectors",
        ] {
            assert!(tables.contains(&t), "missing sealed table {t}");
        }
    }
}
