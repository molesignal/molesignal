# Cipher Keys Capability

## Purpose

加密密钥（AES-256-GCM）的持久化、CRUD、轮换，以及 pipeline 内的 `encrypt` / `decrypt` VRL 函数用于字段级敏感数据保护。

## Requirements

### Requirement: Cipher Key Storage and CRUD

The system SHALL persist cipher keys in a `cipher_keys { id, org_id, name, alg: "aes-256-gcm", version, key_material_enc (bytea), created_at, rotated_at? }` table. Key material at rest SHALL be encrypted with a process-global cipher root key supplied via `MS_CIPHER_KEY` env var (base64 32 bytes); absence of the cipher root key SHALL fail `bootstrap::build_state`. `GET/POST/PUT/DELETE /api/v1/cipher_keys` (Owner only) manage rows; raw key material is never returned, only `id, name, alg, version, created_at, rotated_at`.

#### Scenario: Create cipher key
- **WHEN** an Owner POSTs `{ name: "pii-key" }`
- **THEN** the server generates a fresh 256-bit key, encrypts it with the cipher root key, stores it, and returns `{ id, name, alg, version: 1, ... }` (no raw key)

#### Scenario: Missing cipher root key blocks startup
- **WHEN** `MS_CIPHER_KEY` is empty or unset
- **THEN** `main()` returns `Err("MS_CIPHER_KEY required for cipher_keys")` before any role starts

#### Scenario: Key rotation bumps version
- **WHEN** an Owner PUTs `?rotate=true` to a key
- **THEN** a new 256-bit key is generated, `version = previous + 1`, `rotated_at = now`; previous version is retained so older ciphertexts remain decryptable

### Requirement: Cipher root key naming

The system SHALL use the name `CipherRootKey` (struct) + `MS_CIPHER_KEY` (environment variable) + `cipher_root_key.rs` (module file) for the envelope KEK that wraps user-level cipher_keys table rows. The legacy name `MasterKey` / `MS_MASTER_KEY` / `master_key.rs` SHALL be removed entirely (no backward-compatible alias).

#### Scenario: Process reads env var by new name

- **WHEN** `MS_CIPHER_KEY=<32B base64>` is set and the process starts
- **THEN** `CipherRootKey::from_env()` succeeds and the value is loaded
- **AND** any reference to `MS_MASTER_KEY` in env is ignored (not read)

#### Scenario: Missing env triggers dev fallback

- **WHEN** `MS_CIPHER_KEY` is unset
- **THEN** the wire layer logs `tracing::warn!` and falls back to 32-byte all-zero key (dev only)
- **AND** the existing cipher-keys table still functions (all data encrypted with the zero key is decryptable)

### Requirement: Error type and module-file rename

The error type SHALL be renamed `CipherRootKeyError` (was `MasterKeyError`). The module file `crates/infra/src/cipher/master_key.rs` SHALL be moved to `cipher_root_key.rs`. All imports across the workspace SHALL be updated.

#### Scenario: Compile succeeds with no master_key references

- **WHEN** `cargo build --workspace` is run
- **THEN** the build completes with zero references to `master_key` / `MasterKey` / `MasterKeyError` in source code (test files documented as exceptions, see allowlist)

### Requirement: VRL encrypt/decrypt Functions

`pipeline` Function runtimes SHALL register two built-in VRL functions: `encrypt(value, key_id)` returning AES-256-GCM ciphertext (base64) prefixed with `kid:<id>:v<n>:` and `decrypt(value, key_id)` decoding such ciphertexts. Decryption SHALL try the version embedded in the prefix and fall back to current+1 prior versions before failing.

#### Scenario: Pipeline encrypts field
- **WHEN** a VRL pipeline step is `.ssn = encrypt(.ssn, "pii-key")`
- **THEN** the stored `ssn` field on each event is a `kid:...:v1:...` string; the original is unrecoverable without the key

#### Scenario: Decrypt rotated ciphertext
- **WHEN** a value was encrypted under `v1` and the key has rotated to `v2`
- **THEN** `decrypt(value, "pii-key")` still returns the plaintext (version is read from the prefix)
