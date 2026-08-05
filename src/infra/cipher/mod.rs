// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Cipher keys + envelope KEK（命名调整见 auth-hardening）。
//!
//! - [`keys::root::CipherRootKey`]：`MS_CIPHER_KEY` env 读 32B base64；
//!   缺失时由 wire 决定 fail-fast 或 dev fallback（全零 KEK）。
//! - [`keys`]：org-scoped 字段级加密 key（AES-256-GCM 包），版本管理 +
//!   rotation；列存的 `key_material_enc` 是 root key 包过的 ciphertext。
//! - VRL 内置 `encrypt(value, key_id)` / `decrypt(value, key_id)` 由 pipeline 调用，
//!   payload 自描述前缀 `kid:<id>:v<n>:<base64(nonce||ct||tag)>`。

pub mod field;
pub mod kek_rewrap;
pub mod keys;
pub mod payload;

pub use field::{
    FIELD_DEFAULT_KEY_NAME, FieldKeyService, OrgFieldKey, decrypt_field, encrypt_field,
};
pub use keys::{
    CipherKey, CipherKeyRepository, PgCipherKeyRepository,
    root::{CipherRootKey, CipherRootKeyError},
};
pub use payload::{CipherPayload, payload_decode, payload_encode};
