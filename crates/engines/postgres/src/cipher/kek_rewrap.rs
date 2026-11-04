// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! KEK（根密钥）轮换：离线重包所有被 `CipherRootKey` 信封包过的列。
//!
//! 换 `MS_CIPHER_KEY` 时，DB 里所有用旧 KEK seal 的 `(nonce, ciphertext)` 必须用旧 KEK 解、
//! 新 KEK 重封——否则换 key 后这些数据不可解。用根密钥算的摘要（`org_hmac_sha256`）也一样，
//! 不重算就再也按原值查不到这一行。本模块用一份**显式表清单** [`SPECS`] 覆盖这些列，逐行
//! open(old) → seal(new) → UPDATE，返回每表重包行数供核对。
//!
//! **全部表在一个事务里**：中途失败什么都不会改，修好原因后用同样的参数重跑即可。事务一开始就
//! 锁住清单里的表，提交时这些表的每一行都在新 KEK 下，不会漏掉运行期间写入的行。
//!
//! **离线运维操作**：应在停写窗口执行；新 KEK 已就位（`MS_CIPHER_KEY`）、旧 KEK 由调用方提供；
//! 跑完核对各表计数后方可下线旧 KEK。字段数据本身不动（只换信封）。
//!
//! 根密钥还有三处用途不在这份清单里，轮换时要另外处理：
//! - `[wal].encrypt` 打开时，WAL 用根密钥封在节点磁盘上：轮换前节点要把 WAL 落完；
//! - `derive_key` 的派生密钥直接换掉：通知回调地址里的 Token 会变，外部平台上要重新配置；
//! - 字段脱敏的 `hash` 在查询时现算：轮换前后同一个值的结果不同。

use sqlx::{PgConnection, PgPool, Row};

use super::keys::root::CipherRootKey;
use crate::{
    infra::persistence::sqlx_err,
    shared::{Error, Result},
};

/// 一对 `(nonce, ciphertext)` 列。
struct SealedPair {
    nonce: &'static str,
    ciphertext: &'static str,
    /// 存这个明文 `org_hmac_sha256` 的列，组织取自同一行的 `org_id`。
    hash: Option<&'static str>,
}

const fn pair(nonce: &'static str, ciphertext: &'static str) -> SealedPair {
    SealedPair {
        nonce,
        ciphertext,
        hash: None,
    }
}

const fn hashed_pair(
    nonce: &'static str,
    ciphertext: &'static str,
    hash: &'static str,
) -> SealedPair {
    SealedPair {
        nonce,
        ciphertext,
        hash: Some(hash),
    }
}

/// 一张表里一组 KEK-sealed 列的重包规格。
struct RewrapSpec {
    table: &'static str,
    /// 主键列（定位行做 UPDATE），比较时统一转 TEXT。
    pk: &'static [&'static str],
    /// 可空的对在行内缺一即跳过（如未配的 secret）。
    pairs: &'static [SealedPair],
}

impl RewrapSpec {
    fn has_hash(&self) -> bool {
        self.pairs.iter().any(|sealed| sealed.hash.is_some())
    }
}

/// 全部 KEK-sealed 列清单（与各 repo 的 seal/open 落库一一对应）。
/// 测试拿它对照迁移文件里的每个 BYTEA 列，新增的列必须进这里或声明为不重包。
const SPECS: &[RewrapSpec] = &[
    RewrapSpec {
        table: "cipher_keys",
        pk: &["id"],
        pairs: &[pair("nonce", "key_material_enc")],
    },
    RewrapSpec {
        table: "sealed_secrets",
        pk: &["org_id", "owner_kind", "owner_id", "slot"],
        pairs: &[pair("nonce", "ciphertext")],
    },
    RewrapSpec {
        table: "api_tokens",
        pk: &["id"],
        pairs: &[pair("plaintext_nonce", "plaintext_sealed")],
    },
    RewrapSpec {
        table: "resource_shares",
        pk: &["id"],
        pairs: &[pair("token_nonce", "token_ciphertext")],
    },
    RewrapSpec {
        table: "notify_connectors",
        pk: &["id"],
        pairs: &[pair("config_nonce", "config_ciphertext")],
    },
    RewrapSpec {
        table: "connectors",
        pk: &["id"],
        pairs: &[pair("secrets_nonce", "secrets_ciphertext")],
    },
    RewrapSpec {
        table: "alert_source_mailboxes",
        pk: &["source_id"],
        pairs: &[pair("secret_nonce", "secret_ciphertext")],
    },
    RewrapSpec {
        table: "ticket_connectors",
        pk: &["id"],
        pairs: &[pair("secret_nonce", "secret_ciphertext")],
    },
    RewrapSpec {
        table: "org_settings",
        pk: &["org_id", "section"],
        pairs: &[pair("secret_nonce", "secret_ciphertext")],
    },
    RewrapSpec {
        table: "remediation_actions",
        pk: &["id"],
        pairs: &[pair("secret_nonce", "secret_ciphertext")],
    },
    RewrapSpec {
        table: "kubernetes_clusters",
        pk: &["id"],
        pairs: &[pair("token_nonce", "token_ciphertext")],
    },
    RewrapSpec {
        table: "synthetic_secret_versions",
        pk: &["secret_id", "version"],
        pairs: &[pair("nonce", "ciphertext")],
    },
    RewrapSpec {
        table: "synthetic_probe_certificate_authorities",
        pk: &["id"],
        pairs: &[pair("private_key_nonce", "private_key_ciphertext")],
    },
    RewrapSpec {
        table: "synthetic_monitor_revisions",
        pk: &["id"],
        pairs: &[pair("client_key_nonce", "client_key_ciphertext")],
    },
    RewrapSpec {
        table: "status_page_subscribers",
        pk: &["id"],
        pairs: &[
            hashed_pair("target_nonce", "target_ciphertext", "target_hash"),
            pair("token_nonce", "token_ciphertext"),
        ],
    },
    RewrapSpec {
        table: "status_page_access_rules",
        pk: &["id"],
        pairs: &[hashed_pair("value_nonce", "value_ciphertext", "value_hash")],
    },
    RewrapSpec {
        table: "status_page_magic_links",
        pk: &["id"],
        pairs: &[hashed_pair("email_nonce", "email_ciphertext", "email_hash")],
    },
    RewrapSpec {
        table: "status_page_access_sessions",
        pk: &["id"],
        pairs: &[hashed_pair("email_nonce", "email_ciphertext", "email_hash")],
    },
    RewrapSpec {
        table: "users_ext",
        pk: &["user_id"],
        pairs: &[pair("totp_secret_nonce", "totp_secret_ciphertext")],
    },
    RewrapSpec {
        table: "one_time_tokens",
        pk: &["token_hash"],
        pairs: &[pair("secret_nonce", "secret_ciphertext")],
    },
    RewrapSpec {
        table: "signing_secrets",
        pk: &["id"],
        pairs: &[pair("secret_nonce", "secret_ciphertext")],
    },
];

/// 旧 KEK 解开一个信封。错误只带原因，哪张表哪一行由调用方补上。
fn open_with_old(
    old: &CipherRootKey,
    new: &CipherRootKey,
    nonce: &[u8],
    ct: &[u8],
) -> Result<Vec<u8>, String> {
    old.open(nonce, ct).map_err(|e| {
        // 重跑时最可能的情况是上一次其实已经提交。说清楚，免得运维以为旧 KEK 给错了。
        if new.open(nonce, ct).is_ok() {
            "already sealed with the new KEK: this command has finished before, \
             or a node has written the row with the new KEK"
                .to_string()
        } else {
            format!("rewrap open with old KEK: {e}")
        }
    })
}

/// 行里一对 `(nonce, ciphertext)` 列的值。
type StoredPair = (Option<Vec<u8>>, Option<Vec<u8>>);

/// 一行要写回的新值，顺序就是 UPDATE 里 SET 的顺序：先信封，后摘要。
#[derive(Default)]
struct RewrappedRow {
    sealed: Vec<(&'static str, Vec<u8>)>,
    hashes: Vec<(&'static str, String)>,
}

impl RewrappedRow {
    fn set_cols(&self) -> Vec<&'static str> {
        self.sealed
            .iter()
            .map(|(column, _)| *column)
            .chain(self.hashes.iter().map(|(column, _)| *column))
            .collect()
    }
}

/// 重包一行：`sealed` 与 `spec.pairs` 一一对应，`org_id` 只有带摘要的表才有。
fn rewrap_row(
    old: &CipherRootKey,
    new: &CipherRootKey,
    spec: &RewrapSpec,
    org_id: Option<&str>,
    sealed: &[StoredPair],
) -> Result<RewrappedRow, String> {
    let mut fresh = RewrappedRow::default();

    for (columns, values) in spec.pairs.iter().zip(sealed) {
        let (Some(nonce), Some(ct)) = values else {
            continue;
        };

        let raw = open_with_old(old, new, nonce, ct)
            .map_err(|e| format!("{}: {e}", columns.ciphertext))?;

        let (new_nonce, new_ct) = new
            .seal(&raw)
            .map_err(|e| format!("{}: rewrap seal with new KEK: {e}", columns.ciphertext))?;

        fresh.sealed.push((columns.nonce, new_nonce));
        fresh.sealed.push((columns.ciphertext, new_ct));

        if let Some(hash) = columns.hash {
            let org_id = org_id.ok_or_else(|| format!("{hash}: no org_id"))?;

            fresh.hashes.push((hash, new.org_hmac_sha256(org_id, &raw)));
        }
    }

    Ok(fresh)
}

fn lock_sql() -> String {
    let tables: Vec<&str> = SPECS.iter().map(|spec| spec.table).collect();

    format!(
        "LOCK TABLE {} IN SHARE ROW EXCLUSIVE MODE",
        tables.join(", ")
    )
}

fn select_sql(spec: &RewrapSpec) -> String {
    let mut cols: Vec<String> = spec
        .pk
        .iter()
        .map(|column| format!("{column}::TEXT AS {column}"))
        .collect();

    if spec.has_hash() {
        cols.push("org_id".to_string());
    }

    for sealed in spec.pairs {
        cols.push(sealed.nonce.to_string());
        cols.push(sealed.ciphertext.to_string());
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
            let s = format!("{c}::TEXT = ${idx}");
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
    connection: &mut PgConnection,
    old: &CipherRootKey,
    new: &CipherRootKey,
    spec: &RewrapSpec,
) -> Result<usize> {
    let rows = sqlx::query(&select_sql(spec))
        .fetch_all(&mut *connection)
        .await
        .map_err(sqlx_err)?;

    let mut updated = 0usize;

    for row in &rows {
        let pk_vals: Vec<String> = spec
            .pk
            .iter()
            .map(|c| row.try_get::<String, _>(*c).map_err(sqlx_err))
            .collect::<Result<_>>()?;

        let org_id: Option<String> = if spec.has_hash() {
            Some(row.try_get("org_id").map_err(sqlx_err)?)
        } else {
            None
        };

        let sealed = spec
            .pairs
            .iter()
            .map(|columns| {
                Ok((
                    row.try_get(columns.nonce).map_err(sqlx_err)?,
                    row.try_get(columns.ciphertext).map_err(sqlx_err)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;

        // 报错要点名这一行：事务里一行解不开就全部作废，运维得知道要处理哪一行。
        let row_name = format!("{} {}", spec.table, pk_vals.join("/"));

        let fresh = rewrap_row(old, new, spec, org_id.as_deref(), &sealed)
            .map_err(|e| Error::invalid(format!("{row_name}: {e}")))?;

        let set_cols = fresh.set_cols();

        if set_cols.is_empty() {
            continue; // 该行无（已配的）密文列
        }

        let sql = update_sql(spec.table, &set_cols, spec.pk);
        let mut q = sqlx::query(&sql);

        for (_, v) in &fresh.sealed {
            q = q.bind(v.as_slice());
        }

        for (_, v) in &fresh.hashes {
            q = q.bind(v.as_str());
        }

        for pk in &pk_vals {
            q = q.bind(pk.as_str());
        }

        let done = q.execute(&mut *connection).await.map_err(sqlx_err)?;

        // 漏掉一行就等于丢了这行的密文，报告里还看不出来。
        if done.rows_affected() != 1 {
            return Err(Error::internal(format!(
                "{row_name}: updated {} rows",
                done.rows_affected()
            )));
        }

        updated += 1;
    }

    Ok(updated)
}

/// 重包全部 KEK-sealed 列，返回 `(table, rewrapped_rows)` 列表供运维核对。
///
/// 返回错误时事务没有提交，库里还是旧 KEK，可以重跑。
pub async fn rewrap_all(
    pool: &PgPool,
    old: &CipherRootKey,
    new: &CipherRootKey,
) -> Result<Vec<(String, usize)>> {
    let mut transaction = sqlx::begin(pool).await.map_err(sqlx_err)?;

    sqlx::query(&lock_sql())
        .execute(&mut *transaction)
        .await
        .map_err(sqlx_err)?;

    let mut report = Vec::with_capacity(SPECS.len());

    for spec in SPECS {
        let n = rewrap_table(&mut transaction, old, new, spec).await?;
        report.push((spec.table.to_string(), n));
    }

    transaction.commit().await.map_err(sqlx_err)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, BTreeSet},
        path::{Path, PathBuf},
        sync::LazyLock,
    };

    use base64::Engine as _;
    use regex::Regex;

    use super::*;

    /// 迁移文件里有、但不是根密钥信封的 BYTEA 列。
    /// 新增 BYTEA 列时，要么进 [`SPECS`]，要么写在这里并说明为什么不用重包。
    const NOT_SEALED: &[(&str, &str)] = &[
        // 通行密钥的凭据 id，本来就是公开的。
        ("user_passkeys", "credential_id"),
        // 探针的公钥。
        ("synthetic_probe_agents", "public_key_der"),
        // 普通摘要，不用根密钥算，也不可逆。
        ("synthetic_probe_register_tokens", "token_hash"),
        ("synthetic_probe_agent_tokens", "token_hash"),
        ("synthetic_monitor_revisions", "heartbeat_token_hash"),
        ("synthetic_result_artifacts", "sha256"),
        // APM 分桶的维度键：维度的 BLAKE3 摘要。
        ("apm_buckets", "dimension_key"),
        ("apm_buckets_hourly", "dimension_key"),
    ];

    /// 调 `org_hmac_sha256` 的文件。结果存库的，它的列要写进 [`SPECS`]，否则换根密钥后查不到。
    const ROOT_KEYED_HASH_CALLERS: &[&str] = &[
        // 查询时现算，不存库。
        "bin/molesignal/src/infra/masking/field.rs",
        // value_hash、email_hash。
        "crates/engines/postgres/src/persistence/repositories/status_pages/access.rs",
        // target_hash。
        "crates/engines/postgres/src/persistence/repositories/status_pages/subscriptions.rs",
    ];

    /// `derive_key` 的全部用途。派生出的值没法重包，结果存库或交给外部的，要写进模块说明。
    const DERIVED_KEY_PURPOSES: &[&str] = &[
        // MCP 请求状态，只活在一次请求往返里。
        "inbound-mcp/request-state/v1",
        // 通知回调地址里的 Token，轮换后外部平台要重新配置。
        "notify-reply-callback/v1",
    ];

    fn kek(byte: u8) -> CipherRootKey {
        let b64 = base64::engine::general_purpose::STANDARD.encode([byte; 32]);
        CipherRootKey::from_base64(&b64).unwrap()
    }

    fn spec_of(table: &str) -> &'static RewrapSpec {
        SPECS.iter().find(|spec| spec.table == table).unwrap()
    }

    #[test]
    fn rewrap_row_round_trips_old_to_new() {
        let old = kek(1);
        let new = kek(2);
        let (nonce, ct) = old.seal(b"super-secret").unwrap();
        let spec = spec_of("sealed_secrets");
        let fresh = rewrap_row(&old, &new, spec, None, &[(Some(nonce), Some(ct))]).unwrap();

        assert_eq!(fresh.set_cols(), ["nonce", "ciphertext"]);
        assert!(fresh.hashes.is_empty());

        // 新信封用新 KEK 解得回原文；旧 KEK 不再能解新信封。
        let (new_nonce, new_ct) = (&fresh.sealed[0].1, &fresh.sealed[1].1);
        assert_eq!(new.open(new_nonce, new_ct).unwrap(), b"super-secret");
        assert!(old.open(new_nonce, new_ct).is_err());
    }

    #[test]
    fn rewrap_row_recomputes_the_root_keyed_hash_of_the_value_it_belongs_to() {
        let old = kek(1);
        let new = kek(2);
        let (target_nonce, target_ct) = old.seal(b"a@example.com").unwrap();
        let (token_nonce, token_ct) = old.seal(b"raw-token").unwrap();
        let spec = spec_of("status_page_subscribers");

        let fresh = rewrap_row(
            &old,
            &new,
            spec,
            Some("org-a"),
            &[
                (Some(target_nonce), Some(target_ct)),
                (Some(token_nonce), Some(token_ct)),
            ],
        )
        .unwrap();

        assert_eq!(
            fresh.set_cols(),
            [
                "target_nonce",
                "target_ciphertext",
                "token_nonce",
                "token_ciphertext",
                "target_hash"
            ]
        );

        let hash = new.org_hmac_sha256("org-a", b"a@example.com");
        assert_eq!(fresh.hashes[0].1, hash);
        assert_ne!(hash, old.org_hmac_sha256("org-a", b"a@example.com"));
        assert_ne!(hash, new.org_hmac_sha256("org-b", b"a@example.com"));
    }

    #[test]
    fn rewrap_row_skips_a_pair_that_is_not_set() {
        let old = kek(1);
        let new = kek(2);
        let spec = spec_of("api_tokens");

        for sealed in [
            (None, None),
            (Some(vec![0; 12]), None),
            (None, Some(vec![0; 17])),
        ] {
            let fresh = rewrap_row(&old, &new, spec, None, &[sealed]).unwrap();
            assert!(fresh.set_cols().is_empty());
        }
    }

    #[test]
    fn rewrap_row_rejects_a_wrong_old_key_and_names_the_column() {
        let real_old = kek(1);
        let wrong_old = kek(9);
        let new = kek(2);
        let (nonce, ct) = real_old.seal(b"x").unwrap();
        let spec = spec_of("cipher_keys");

        let error = rewrap_row(&wrong_old, &new, spec, None, &[(Some(nonce), Some(ct))])
            .err()
            .unwrap();

        assert!(error.contains("key_material_enc"), "{error}");
        assert!(error.contains("rewrap open with old KEK"), "{error}");
    }

    #[test]
    fn rewrap_row_says_when_a_row_is_already_under_the_new_key() {
        let old = kek(1);
        let new = kek(2);
        let (nonce, ct) = new.seal(b"x").unwrap();
        let spec = spec_of("cipher_keys");

        let error = rewrap_row(&old, &new, spec, None, &[(Some(nonce), Some(ct))])
            .err()
            .unwrap();

        assert!(error.contains("already sealed with the new KEK"), "{error}");
    }

    #[test]
    fn select_sql_lists_pk_then_pairs() {
        assert_eq!(
            select_sql(spec_of("cipher_keys")),
            "SELECT id::TEXT AS id, nonce, key_material_enc FROM cipher_keys"
        );
    }

    #[test]
    fn select_sql_reads_the_organization_of_a_hashed_table() {
        assert_eq!(
            select_sql(spec_of("status_page_access_rules")),
            "SELECT id::TEXT AS id, org_id, value_nonce, value_ciphertext \
             FROM status_page_access_rules"
        );
    }

    #[test]
    fn update_sql_numbers_set_then_where() {
        let sql = update_sql(
            "sealed_secrets",
            &["nonce", "ciphertext"],
            &["org_id", "owner_id"],
        );
        assert_eq!(
            sql,
            "UPDATE sealed_secrets SET nonce = $1, ciphertext = $2 WHERE org_id::TEXT = $3 AND owner_id::TEXT = $4"
        );
    }

    #[test]
    fn lock_sql_names_every_table_in_one_statement() {
        let sql = lock_sql();

        for spec in SPECS {
            assert!(sql.contains(spec.table), "{} is not locked", spec.table);
        }

        assert!(sql.starts_with("LOCK TABLE cipher_keys, "), "{sql}");
        assert!(sql.ends_with(" IN SHARE ROW EXCLUSIVE MODE"), "{sql}");
    }

    #[test]
    fn specs_list_each_table_once() {
        let tables: BTreeSet<&str> = SPECS.iter().map(|spec| spec.table).collect();
        assert_eq!(tables.len(), SPECS.len());
    }

    /// 防过时：迁移跑完后的每个 BYTEA 列，要么在清单里，要么声明了不重包，并且两边都没有多余的。
    #[test]
    fn every_binary_column_is_rewrapped_or_declared_not_sealed() {
        let tables = migrated_tables();

        let binary: BTreeSet<(String, String)> = tables
            .iter()
            .flat_map(|(table, columns)| {
                columns
                    .columns
                    .iter()
                    .filter(|(_, kind)| kind.as_str() == "BYTEA")
                    .map(|(column, _)| (table.clone(), column.clone()))
            })
            .collect();

        let mut known: BTreeSet<(String, String)> = NOT_SEALED
            .iter()
            .map(|(table, column)| (table.to_string(), column.to_string()))
            .collect();

        for spec in SPECS {
            for sealed in spec.pairs {
                known.insert((spec.table.to_string(), sealed.nonce.to_string()));
                known.insert((spec.table.to_string(), sealed.ciphertext.to_string()));
            }
        }

        let unknown: Vec<_> = binary.difference(&known).collect();
        let stale: Vec<_> = known.difference(&binary).collect();

        assert!(
            unknown.is_empty(),
            "BYTEA columns neither in SPECS nor in NOT_SEALED: {unknown:?}"
        );
        assert!(
            stale.is_empty(),
            "SPECS or NOT_SEALED name columns the migrations no longer have: {stale:?}"
        );
    }

    /// 根密钥的密文只能存在 BYTEA 里，上面那条检查才算数。
    #[test]
    fn columns_named_like_sealed_data_are_binary() {
        let sealed_name = Regex::new(r"(?i)(ciphertext|nonce|_sealed$|_enc$)").unwrap();

        for (table, columns) in migrated_tables() {
            for (column, kind) in &columns.columns {
                assert!(
                    !sealed_name.is_match(column) || kind == "BYTEA",
                    "{table}.{column} is named like sealed data but is {kind}"
                );
            }
        }
    }

    /// `pk` 不是真主键时，一条 UPDATE 会把几行写成同一个密文。
    #[test]
    fn specs_name_the_real_primary_key_and_hash_columns() {
        let tables = migrated_tables();

        for spec in SPECS {
            let table = tables.get(spec.table).unwrap_or_else(|| {
                panic!("{} is not a table after the migrations", spec.table);
            });

            assert_eq!(table.primary_key, spec.pk, "primary key of {}", spec.table);

            for sealed in spec.pairs {
                if let Some(hash) = sealed.hash {
                    assert_eq!(
                        table.columns.get(hash).map(String::as_str),
                        Some("CHAR(64)"),
                        "{}.{hash}",
                        spec.table
                    );
                    assert!(table.columns.contains_key("org_id"), "{}", spec.table);
                }
            }
        }
    }

    #[test]
    fn root_keyed_hashes_are_only_computed_where_the_specs_expect() {
        let call = Regex::new(r"\.org_hmac_sha256\(").unwrap();

        let callers: BTreeSet<String> = workspace_sources()
            .into_iter()
            .filter(|(_, source)| call.is_match(source))
            .map(|(path, _)| path)
            .collect();

        let known: BTreeSet<String> = ROOT_KEYED_HASH_CALLERS
            .iter()
            .map(|path| path.to_string())
            .collect();

        assert_eq!(callers, known);
    }

    #[test]
    fn keys_derived_from_the_root_key_are_known() {
        let call = Regex::new(r#"\.derive_key\(\s*b"([^"]+)""#).unwrap();

        let purposes: BTreeSet<String> = workspace_sources()
            .iter()
            .flat_map(|(_, source)| call.captures_iter(source))
            .map(|found| found[1].to_string())
            .collect();

        let known: BTreeSet<String> = DERIVED_KEY_PURPOSES
            .iter()
            .map(|purpose| purpose.to_string())
            .collect();

        assert_eq!(purposes, known);
    }

    #[test]
    fn schema_reader_follows_renames_drops_and_late_columns() {
        let tables = schema_of(
            "-- CREATE TABLE commented_out (a BYTEA);
             CREATE TABLE IF NOT EXISTS old_name (
                 id VARCHAR(64) PRIMARY KEY,
                 kind VARCHAR(16) NOT NULL CHECK (kind IN ('a,b', 'c;d')),
                 nonce BYTEA,
                 gone BYTEA,
                 CONSTRAINT chk_kind CHECK (octet_length(nonce) = 12)
             );
             ALTER TABLE old_name RENAME TO new_name;
             ALTER TABLE new_name
                 ADD COLUMN IF NOT EXISTS late BYTEA NOT NULL,
                 ADD COLUMN later TEXT,
                 ADD CONSTRAINT chk_later CHECK (later <> ''),
                 DROP COLUMN gone;
             ALTER TABLE new_name RENAME COLUMN nonce TO renamed;
             CREATE TABLE pairs (a TEXT, b INTEGER, payload BYTEA, PRIMARY KEY (a, b));
             CREATE TABLE dropped (a BYTEA);
             DROP TABLE IF EXISTS dropped;
             CREATE TEMP TABLE seed (a BYTEA);
             CREATE FUNCTION f() RETURNS VOID AS $body$
                 BEGIN CREATE TABLE in_a_function (a BYTEA); END;
             $body$ LANGUAGE plpgsql;
             INSERT INTO pairs (a) VALUES ('; CREATE TABLE in_a_string (a BYTEA);');",
        );

        assert_eq!(
            tables.keys().map(String::as_str).collect::<Vec<_>>(),
            ["new_name", "pairs"]
        );

        let renamed = &tables["new_name"];
        assert_eq!(renamed.primary_key, ["id"]);
        assert_eq!(renamed.columns["late"], "BYTEA");
        assert_eq!(renamed.columns["renamed"], "BYTEA");
        assert_eq!(renamed.columns["later"], "TEXT");
        assert!(!renamed.columns.contains_key("gone"));
        assert!(!renamed.columns.contains_key("nonce"));
        assert_eq!(tables["pairs"].primary_key, ["a", "b"]);
    }

    #[test]
    fn schema_reader_sees_the_real_baseline() {
        let tables = migrated_tables();

        assert!(!tables.contains_key("ai_prompt_templates"));
        assert!(tables.contains_key("agent_prompt_templates"));
        assert!(tables.contains_key("sealed_secrets"));
        assert!(tables.len() > 200, "only read {} tables", tables.len());
    }

    /// 迁移跑完后的一张表。
    #[derive(Default)]
    struct MigratedTable {
        /// 列名 → 类型的第一个词，大写。
        columns: BTreeMap<String, String>,
        primary_key: Vec<String>,
    }

    fn migrated_tables() -> BTreeMap<String, MigratedTable> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/migrations");

        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "sql"))
            .collect();

        files.sort();

        let sql: Vec<String> = files
            .iter()
            .map(|path| std::fs::read_to_string(path).unwrap())
            .collect();

        schema_of(&sql.join("\n;\n"))
    }

    /// 按顺序读 SQL，算出跑完之后有哪些表。函数体（`$tag$` 块）和 `DO` 块里的 SQL 不算数：
    /// 里面动态建的表不放根密钥的密文。
    fn schema_of(sql: &str) -> BTreeMap<String, MigratedTable> {
        static CREATE: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?is)^\s*CREATE\s+TABLE\s+(IF\s+NOT\s+EXISTS\s+)?(?:public\.)?(\w+)\s*\(")
                .unwrap()
        });
        static ALTER: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(
                r"(?is)^\s*ALTER\s+TABLE\s+(?:IF\s+EXISTS\s+)?(?:ONLY\s+)?(?:public\.)?(\w+)\s",
            )
            .unwrap()
        });
        static DROP: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(
                r"(?is)^\s*DROP\s+TABLE\s+(?:IF\s+EXISTS\s+)?([\w\s,.]+?)\s*(?:CASCADE)?\s*$",
            )
            .unwrap()
        });

        let mut tables = BTreeMap::new();

        for statement in sql_statements(sql) {
            if let Some(found) = CREATE.captures(&statement) {
                let name = found[2].to_lowercase();
                let mut table = MigratedTable::default();

                for item in top_level_items(until_closing_paren(&statement[found[0].len()..])) {
                    add_table_item(&mut table, item);
                }

                if found.get(1).is_none() || !tables.contains_key(&name) {
                    tables.insert(name, table);
                }
            } else if let Some(found) = ALTER.captures(&statement) {
                let mut name = found[1].to_lowercase();

                for action in top_level_items(&statement[found[0].len()..]) {
                    apply_alter(&mut tables, &mut name, action);
                }
            } else if let Some(found) = DROP.captures(&statement) {
                for name in found[1].split(',') {
                    tables.remove(&name.trim().to_lowercase());
                }
            }
        }

        tables
    }

    fn add_table_item(table: &mut MigratedTable, item: &str) {
        static TABLE_KEY: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?is)^\s*(?:CONSTRAINT\s+\w+\s+)?PRIMARY\s+KEY\s*\(([^)]*)\)").unwrap()
        });
        static CONSTRAINT: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?i)^\s*(CONSTRAINT|UNIQUE|CHECK|FOREIGN|EXCLUDE|LIKE)\b").unwrap()
        });
        static INLINE_KEY: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"(?i)\bPRIMARY\s+KEY\b").unwrap());

        if let Some(found) = TABLE_KEY.captures(item) {
            table.primary_key = found[1]
                .split(',')
                .map(|column| column.trim().to_lowercase())
                .collect();
        } else if !CONSTRAINT.is_match(item) {
            let mut words = item.split_whitespace();

            if let (Some(name), Some(kind)) = (words.next(), words.next()) {
                let name = name.trim_matches('"').to_lowercase();

                if INLINE_KEY.is_match(item) {
                    table.primary_key = vec![name.clone()];
                }

                table.columns.insert(name, kind.to_uppercase());
            }
        }
    }

    fn apply_alter(tables: &mut BTreeMap<String, MigratedTable>, name: &mut String, action: &str) {
        static RENAME_TABLE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"(?is)^\s*RENAME\s+TO\s+(\w+)\s*$").unwrap());
        static RENAME_COLUMN: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?is)^\s*RENAME\s+(?:COLUMN\s+)?(\w+)\s+TO\s+(\w+)\s*$").unwrap()
        });
        static ADD: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?is)^\s*ADD\s+(?:COLUMN\s+)?(?:IF\s+NOT\s+EXISTS\s+)?").unwrap()
        });
        static DROP_COLUMN: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?is)^\s*DROP\s+COLUMN\s+(?:IF\s+EXISTS\s+)?(\w+)").unwrap()
        });
        static SET_TYPE: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?is)^\s*ALTER\s+(?:COLUMN\s+)?(\w+)\s+(?:SET\s+DATA\s+)?TYPE\s+(\S+)")
                .unwrap()
        });

        if let Some(found) = RENAME_TABLE.captures(action) {
            if let Some(table) = tables.remove(name.as_str()) {
                *name = found[1].to_lowercase();
                tables.insert(name.clone(), table);
            }

            return;
        }

        let Some(table) = tables.get_mut(name.as_str()) else {
            return;
        };

        if let Some(found) = RENAME_COLUMN.captures(action) {
            let (from, to) = (found[1].to_lowercase(), found[2].to_lowercase());

            if let Some(kind) = table.columns.remove(&from) {
                table.columns.insert(to.clone(), kind);
            }

            for column in &mut table.primary_key {
                if *column == from {
                    *column = to.clone();
                }
            }
        } else if let Some(found) = ADD.find(action) {
            add_table_item(table, &action[found.end()..]);
        } else if let Some(found) = DROP_COLUMN.captures(action) {
            table.columns.remove(&found[1].to_lowercase());
        } else if let Some(found) = SET_TYPE.captures(action) {
            table
                .columns
                .insert(found[1].to_lowercase(), found[2].to_uppercase());
        }
    }

    /// 拆成语句：去掉注释和 `$tag$` 块，字符串只留一对空引号。
    fn sql_statements(sql: &str) -> Vec<String> {
        let mut statements = Vec::new();
        let mut current = String::new();
        let mut rest = sql;

        while let Some(next) = rest.chars().next() {
            if let Some(comment) = rest.strip_prefix("--") {
                rest = comment.split_once('\n').map_or("", |(_, tail)| tail);
            } else if next == '\'' {
                // `''` 转义相当于两个相邻的字符串，不用单独处理。
                rest = rest[1..].split_once('\'').map_or("", |(_, tail)| tail);
                current.push_str("''");
            } else if let Some(tag) = dollar_tag(rest) {
                rest = rest[tag.len()..]
                    .split_once(tag)
                    .map_or("", |(_, tail)| tail);
            } else if next == ';' {
                statements.push(std::mem::take(&mut current));
                rest = &rest[1..];
            } else {
                current.push(next);
                rest = &rest[next.len_utf8()..];
            }
        }

        statements.push(current);
        statements
    }

    fn dollar_tag(sql: &str) -> Option<&str> {
        let tail = sql.strip_prefix('$')?;
        let end = tail.find('$')?;
        let tag = &tail[..end];

        let is_tag = !tag.starts_with(|c: char| c.is_ascii_digit())
            && tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');

        is_tag.then(|| &sql[..end + 2])
    }

    /// `text` 从开括号之后开始，返回到配对的闭括号之前。
    fn until_closing_paren(text: &str) -> &str {
        let mut depth = 1usize;

        for (index, c) in text.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }

            if depth == 0 {
                return &text[..index];
            }
        }

        text
    }

    /// 按不在括号里的逗号拆开。
    fn top_level_items(text: &str) -> Vec<&str> {
        let mut items = Vec::new();
        let mut depth = 0usize;
        let mut start = 0usize;

        for (index, c) in text.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => {
                    items.push(&text[start..index]);
                    start = index + 1;
                }
                _ => {}
            }
        }

        items.push(&text[start..]);
        items
    }

    /// 工作区的生产源码：`(相对路径, 内容)`。根密钥模块自身和测试目录不算调用方。
    fn workspace_sources() -> Vec<(String, String)> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let mut files = Vec::new();

        for directory in ["bin", "crates"] {
            collect_rust_files(&root.join(directory), &mut files);
        }

        files
            .into_iter()
            .map(|path| {
                let source = std::fs::read_to_string(&path).unwrap();

                let relative = path
                    .strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();

                (relative, source)
            })
            .filter(|(path, _)| !path.starts_with("crates/engines/postgres/src/cipher/"))
            .collect()
    }

    fn collect_rust_files(directory: &Path, output: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();

            if path.is_dir() {
                if path.file_name().is_some_and(|name| name != "tests") {
                    collect_rust_files(&path, output);
                }
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                output.push(path);
            }
        }
    }
}
