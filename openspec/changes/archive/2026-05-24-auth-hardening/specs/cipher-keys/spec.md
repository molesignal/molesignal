## ADDED Requirements

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
