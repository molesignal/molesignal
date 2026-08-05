## ADDED Requirements

### Requirement: Cipher Key Storage and CRUD

The system SHALL persist cipher keys in a `cipher_keys { id, org_id, name, alg: "aes-256-gcm", version, key_material_enc (bytea), created_at, rotated_at? }` table. Key material at rest SHALL be encrypted with a process-global master key supplied via `MS_MASTER_KEY` env var (base64 32 bytes); absence of master key SHALL fail `wire::build_state`. `GET/POST/PUT/DELETE /api/v1/cipher_keys` (Owner only) manage rows; raw key material is never returned, only `id, name, alg, version, created_at, rotated_at`.

#### Scenario: Create cipher key
- **WHEN** an Owner POSTs `{ name: "pii-key" }`
- **THEN** the server generates a fresh 256-bit key, encrypts it with the master key, stores it, and returns `{ id, name, alg, version: 1, ... }` (no raw key)

#### Scenario: Missing master key blocks startup
- **WHEN** `MS_MASTER_KEY` is empty or unset
- **THEN** `main()` returns `Err("MS_MASTER_KEY required for cipher_keys")` before any role starts

#### Scenario: Key rotation bumps version
- **WHEN** an Owner PUTs `?rotate=true` to a key
- **THEN** a new 256-bit key is generated, `version = previous + 1`, `rotated_at = now`; previous version is retained so older ciphertexts remain decryptable

### Requirement: VRL encrypt/decrypt Functions

`pipeline` Function runtimes SHALL register two built-in VRL functions: `encrypt(value, key_id)` returning AES-256-GCM ciphertext (base64) prefixed with `kid:<id>:v<n>:` and `decrypt(value, key_id)` decoding such ciphertexts. Decryption SHALL try the version embedded in the prefix and fall back to current+1 prior versions before failing.

#### Scenario: Pipeline encrypts field
- **WHEN** a VRL pipeline step is `.ssn = encrypt(.ssn, "pii-key")`
- **THEN** the stored `ssn` field on each event is a `kid:...:v1:...` string; the original is unrecoverable without the key

#### Scenario: Decrypt rotated ciphertext
- **WHEN** a value was encrypted under `v1` and the key has rotated to `v2`
- **THEN** `decrypt(value, "pii-key")` still returns the plaintext (version is read from the prefix)
