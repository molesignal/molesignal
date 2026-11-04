// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The private key of an HTTP check's client certificate, sealed into the revision that holds it.
//! A spec only says which revision that is, so no spec, task or response carries the key.

use sqlx::Row;

use super::PgSyntheticRepository;
use crate::{
    domain::synthetics::{ClientKey, MonitorRevision, MonitorSpec, SecretMaterial},
    infra::cipher::CipherRootKey,
    shared::{Error, Result, ids::Id},
};

/// A sealed key, as `(nonce, ciphertext)`.
pub(super) type SealedKey = (Vec<u8>, Vec<u8>);

/// What `revision` stores: its spec, holding the key as the revision's own, and the key sealed. A
/// new key is sealed here; a kept one is copied, still sealed, from the revision that holds it.
pub(super) async fn seal(
    connection: &mut sqlx::PgConnection,
    cipher: &CipherRootKey,
    revision: &MonitorRevision,
) -> Result<(MonitorSpec, Option<SealedKey>)> {
    let mut spec = revision.spec.clone();

    let MonitorSpec::Http(http) = &mut spec else {
        return Ok((spec, None));
    };

    let Some(certificate) = &mut http.client_certificate else {
        return Ok((spec, None));
    };

    let sealed = match &certificate.private_key {
        ClientKey::New { pem } => cipher
            .seal(pem.as_bytes())
            .map_err(|error| Error::internal(format!("encrypt client private key: {error}")))?,
        ClientKey::Kept { revision_id } => {
            sealed_key(&mut *connection, &revision.organization_id, revision_id)
                .await?
                .ok_or_else(|| {
                    Error::invalid(format!(
                        "Revision {revision_id} holds no client private key: upload the key again"
                    ))
                })?
        }
    };

    certificate.private_key = ClientKey::Kept {
        revision_id: revision.id.clone(),
    };

    Ok((spec, Some(sealed)))
}

/// The private key `revision_id` holds, unsealed.
pub(super) async fn open(
    repository: &PgSyntheticRepository,
    org_id: &Id,
    revision_id: &Id,
) -> Result<SecretMaterial> {
    let (nonce, ciphertext) = sealed_key(&repository.pool, org_id, revision_id)
        .await?
        .ok_or_else(|| Error::not_found(format!("client private key of Revision {revision_id}")))?;

    let plaintext = repository
        .cipher
        .open(&nonce, &ciphertext)
        .map_err(|error| Error::internal(format!("decrypt client private key: {error}")))?;

    Ok(SecretMaterial(plaintext))
}

async fn sealed_key(
    executor: impl sqlx::PgExecutor<'_>,
    org_id: &Id,
    revision_id: &Id,
) -> Result<Option<SealedKey>> {
    let row = sqlx::query(
        "SELECT client_key_nonce, client_key_ciphertext
         FROM synthetic_monitor_revisions
         WHERE organization_id = $1 AND id = $2",
    )
    .bind(&org_id.0)
    .bind(&revision_id.0)
    .fetch_optional(executor)
    .await
    .map_err(super::sqlx_err)?;

    let Some(row) = row else {
        return Ok(None);
    };

    let nonce: Option<Vec<u8>> = row.try_get("client_key_nonce").map_err(super::sqlx_err)?;

    let ciphertext: Option<Vec<u8>> = row
        .try_get("client_key_ciphertext")
        .map_err(super::sqlx_err)?;

    Ok(nonce.zip(ciphertext))
}
