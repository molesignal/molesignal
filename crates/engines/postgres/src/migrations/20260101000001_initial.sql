-- MoleSignal 初始 schema。
--
-- 首次发布前只维护这一份 schema 文件；开发环境在结构变化后重建数据库。
-- 各段按依赖顺序执行，保留少量同一事务内的 ALTER / RENAME，以避免复制复杂
-- 表定义并确保最终 schema 与开发期验证过的执行序列一致。
--
-- 列类型约定：
--   - 主键 / 外键引用 → VARCHAR(64)（KSUID 或 UUID 字符串）
--   - 时间戳 → BIGINT（微秒，对应 domain TimestampMicros(i64)）
--   - 复杂结构 → JSONB

CREATE EXTENSION IF NOT EXISTS pg_trgm;

-- ============================================================
-- Identity & access
-- ============================================================

CREATE TABLE IF NOT EXISTS organizations (
    id                  VARCHAR(64) PRIMARY KEY,
    name                VARCHAR(255) NOT NULL,
    slug                VARCHAR(64)  NOT NULL UNIQUE,
    disabled            BOOLEAN      NOT NULL DEFAULT FALSE,
    created_at_micros   BIGINT       NOT NULL
);

CREATE TABLE IF NOT EXISTS users (
    id                  VARCHAR(64) PRIMARY KEY,
    email               VARCHAR(255) NOT NULL UNIQUE,
    display_name        VARCHAR(255) NOT NULL,
    password_hash       TEXT         NOT NULL,
    avatar_url          TEXT,
    disabled            BOOLEAN      NOT NULL DEFAULT FALSE,
    status              VARCHAR(16)  NOT NULL DEFAULT 'active',
    created_at_micros   BIGINT       NOT NULL
);

-- 实例级（全局，非 per-org）设置：单行单例，承载控制面与数据入口策略。
CREATE TABLE IF NOT EXISTS instance_settings (
    id                      SMALLINT PRIMARY KEY DEFAULT 1,
    signup_enabled          BOOLEAN  NOT NULL DEFAULT FALSE,
    signup_require_approval BOOLEAN  NOT NULL DEFAULT TRUE,
    -- 服务图数据来源：intake（进程内配对，低延迟）| storage（单例 worker 重算，跨节点正确）。
    service_graph_source            TEXT   NOT NULL DEFAULT 'intake',
    -- 跨集群联邦：cluster_id 非空即启用（事件 source/writer，联邦内唯一）；其余为后台 worker 调优参数。
    federation_cluster_id           TEXT   NOT NULL DEFAULT '',
    federation_drain_interval_secs  BIGINT NOT NULL DEFAULT 10,
    federation_push_batch_size      BIGINT NOT NULL DEFAULT 100,
    federation_seen_events_ttl_secs BIGINT NOT NULL DEFAULT 604800,
    federation_gossip_interval_secs BIGINT NOT NULL DEFAULT 60,
    -- RUM 客户端 IP 识别。Header / 链模式必须同时配置可信代理 CIDR。
    rum_client_ip_mode                TEXT     NOT NULL DEFAULT 'peer',
    rum_client_ip_header              TEXT     NOT NULL DEFAULT '',
    rum_client_ip_trusted_proxy_cidrs TEXT[]   NOT NULL DEFAULT ARRAY[]::TEXT[],
    rum_client_ip_fallback_to_peer    BOOLEAN  NOT NULL DEFAULT TRUE,
    rum_client_ip_allow_private       BOOLEAN  NOT NULL DEFAULT FALSE,
    rum_client_ip_max_chain_length    SMALLINT NOT NULL DEFAULT 16,
    updated_at_micros       BIGINT   NOT NULL DEFAULT 0,
    CONSTRAINT instance_settings_singleton CHECK (id = 1),
    CONSTRAINT instance_settings_rum_client_ip_mode
        CHECK (rum_client_ip_mode IN ('peer', 'header', 'forwarded_chain')),
    CONSTRAINT instance_settings_rum_client_ip_chain_length
        CHECK (rum_client_ip_max_chain_length BETWEEN 1 AND 64)
);
INSERT INTO instance_settings (id) VALUES (1) ON CONFLICT (id) DO NOTHING;

CREATE TABLE IF NOT EXISTS user_preferences (
    user_id                         VARCHAR(64) PRIMARY KEY,
    theme                           VARCHAR(16) NOT NULL DEFAULT 'light',
    density                         VARCHAR(16) NOT NULL DEFAULT 'normal',
    language                        VARCHAR(16) NOT NULL DEFAULT 'en-us',
    default_home_route              TEXT        NOT NULL DEFAULT '/home',
    time_format                     VARCHAR(16) NOT NULL DEFAULT 'iso_24h',
    keyboard_shortcuts_enabled      BOOLEAN     NOT NULL DEFAULT TRUE,
    -- 用户展示时区；空串 = 跟随浏览器本地时区（客户端解析）。存储仍为 UTC，仅影响渲染。
    timezone                        VARCHAR(64) NOT NULL DEFAULT '',
    updated_at_micros               BIGINT      NOT NULL,
    CONSTRAINT chk_user_preferences_theme
        CHECK (theme IN ('dark', 'light')),
    CONSTRAINT chk_user_preferences_density
        CHECK (density IN ('compact', 'normal', 'comfortable')),
    CONSTRAINT chk_user_preferences_language
        CHECK (language IN ('en-us', 'zh-cn')),
    CONSTRAINT chk_user_preferences_time_format
        CHECK (time_format IN ('iso_24h', 'local_12h')),
    CONSTRAINT chk_user_preferences_default_home_route
        CHECK (default_home_route LIKE '/%' AND default_home_route NOT LIKE '//%')
);

CREATE TABLE IF NOT EXISTS memberships (
    user_id             VARCHAR(64) NOT NULL,
    org_id              VARCHAR(64) NOT NULL,
    role                VARCHAR(16) NOT NULL,
    joined_at_micros    BIGINT      NOT NULL,
    PRIMARY KEY (user_id, org_id)
);
CREATE INDEX IF NOT EXISTS idx_memberships_org ON memberships(org_id);

CREATE TABLE IF NOT EXISTS teams (
    id          VARCHAR(64) PRIMARY KEY,
    org_id      VARCHAR(64)  NOT NULL,
    name        VARCHAR(255) NOT NULL,
    member_ids  JSONB        NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_teams_org ON teams(org_id);

CREATE TABLE IF NOT EXISTS iam_roles (
    id                  VARCHAR(64)  PRIMARY KEY,
    org_id              VARCHAR(64)  NOT NULL,
    role_key            VARCHAR(64)  NOT NULL,
    name                VARCHAR(128) NOT NULL,
    description         TEXT         NOT NULL DEFAULT '',
    builtin             BOOLEAN      NOT NULL DEFAULT FALSE,
    created_at_micros   BIGINT       NOT NULL,
    updated_at_micros   BIGINT       NOT NULL,
    CONSTRAINT chk_iam_roles_role_key
        CHECK (role_key ~ '^[a-z][a-z0-9_]{1,63}$')
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_iam_roles_org_key ON iam_roles(org_id, role_key);
CREATE INDEX IF NOT EXISTS idx_iam_roles_org ON iam_roles(org_id);

CREATE TABLE IF NOT EXISTS iam_role_permissions (
    role_id         VARCHAR(64) NOT NULL,
    permission_key  VARCHAR(64) NOT NULL,
    PRIMARY KEY (role_id, permission_key)
);
CREATE INDEX IF NOT EXISTS idx_iam_role_permissions_role ON iam_role_permissions(role_id);

CREATE TABLE IF NOT EXISTS invitations (
    id                  VARCHAR(64)  PRIMARY KEY,
    org_id              VARCHAR(64)  NOT NULL,
    email               VARCHAR(255) NOT NULL,
    role                VARCHAR(32)  NOT NULL,
    inviter_id          VARCHAR(64)  NOT NULL,
    status              VARCHAR(32)  NOT NULL,
    sent_at_micros      BIGINT       NOT NULL,
    updated_at_micros   BIGINT       NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_invitations_org_status
    ON invitations(org_id, status);
CREATE UNIQUE INDEX IF NOT EXISTS uq_invitations_org_email_pending
    ON invitations(org_id, lower(email))
    WHERE status = 'pending';

CREATE TABLE IF NOT EXISTS sso_sessions (
    id                   VARCHAR(64)  PRIMARY KEY,
    user_id              VARCHAR(64)  NOT NULL,
    provider             VARCHAR(16)  NOT NULL,    -- 'oidc' | 'saml'
    idp_subject          VARCHAR(255) NOT NULL,
    issued_at_micros     BIGINT       NOT NULL,
    last_login_at_micros BIGINT       NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_sso_user    ON sso_sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_sso_idp_sub ON sso_sessions(provider, idp_subject);

CREATE TABLE IF NOT EXISTS sso_providers (
    id                  VARCHAR(64)  PRIMARY KEY,
    org_id              VARCHAR(64)  NOT NULL,
    name                VARCHAR(128) NOT NULL,
    provider            VARCHAR(16)  NOT NULL,    -- 'oidc' | 'saml'
    enabled             BOOLEAN      NOT NULL DEFAULT TRUE,
    config              JSONB        NOT NULL,
    created_at_micros   BIGINT       NOT NULL,
    updated_at_micros   BIGINT       NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_sso_providers_org_name
    ON sso_providers(org_id, name);
CREATE INDEX IF NOT EXISTS idx_sso_providers_org_enabled
    ON sso_providers(org_id, enabled);

-- ============================================================
-- Service accounts / API tokens / audit / quotas / signing secrets
-- ============================================================

CREATE TABLE IF NOT EXISTS service_accounts (
    id                  VARCHAR(64)  PRIMARY KEY,
    org_id              VARCHAR(64)  NOT NULL,
    name                VARCHAR(128) NOT NULL,
    description         TEXT         NOT NULL DEFAULT '',
    role_id             VARCHAR(64)  NOT NULL,
    disabled            BOOLEAN      NOT NULL DEFAULT FALSE,
    created_by          VARCHAR(64)  NOT NULL,
    created_at_micros   BIGINT       NOT NULL,
    updated_at_micros   BIGINT       NOT NULL,
    deleted_at_micros   BIGINT
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_service_accounts_org_id
    ON service_accounts(org_id, id);
CREATE UNIQUE INDEX IF NOT EXISTS uq_service_accounts_org_name_active
    ON service_accounts(org_id, lower(name)) WHERE deleted_at_micros IS NULL;
CREATE INDEX IF NOT EXISTS idx_service_accounts_org
    ON service_accounts(org_id, created_at_micros DESC)
    WHERE deleted_at_micros IS NULL;

-- API Token 是独立凭证资源。personal/default 归属用户，rum_client 绑定应用；
-- service_account 类型的 API Token 以不可登录的 Service Account 作为 IAM principal。
CREATE TABLE IF NOT EXISTS api_tokens (
    id                  VARCHAR(64)  PRIMARY KEY,
    org_id              VARCHAR(64)  NOT NULL,
    user_id             VARCHAR(64)  NOT NULL DEFAULT '',
    prefix              VARCHAR(32)  NOT NULL,    -- 仅保存 16 字符可见前缀；展示时按 token_kind 补 ms_ / msrum_
    secret_hash         TEXT         NOT NULL,    -- personal/default: Argon2id; public RUM: SHA-256 over a high-entropy secret
    name                VARCHAR(255) NOT NULL,
    role                VARCHAR(16)  NOT NULL,
    created_at_micros   BIGINT       NOT NULL,
    expires_at_micros   BIGINT,
    last_used_at_micros BIGINT,
    revoked             BOOLEAN      NOT NULL DEFAULT FALSE,
    token_kind          VARCHAR(32)  NOT NULL DEFAULT 'personal',
    application_id      VARCHAR(128),
    service_account_id  VARCHAR(64),
    -- 默认接入 PAT：每个 (org, user) 至多一个活的默认 token，完整明文经 KEK seal 存 plaintext_sealed，可重复回显。
    is_default          BOOLEAN      NOT NULL DEFAULT FALSE,
    plaintext_sealed    BYTEA,
    plaintext_nonce     BYTEA,
    CONSTRAINT chk_api_token_kind
        CHECK (token_kind IN ('personal', 'default_intake', 'rum_client', 'service_account')),
    CONSTRAINT chk_api_token_application
        CHECK (
            (token_kind = 'rum_client'
                AND application_id IS NOT NULL
                AND application_id ~ '^[A-Za-z0-9._:-]{1,128}$')
            OR (token_kind <> 'rum_client' AND application_id IS NULL)
        ),
    CONSTRAINT chk_api_token_service_account
        CHECK (
            (token_kind = 'service_account' AND service_account_id IS NOT NULL)
            OR (token_kind <> 'service_account' AND service_account_id IS NULL)
        ),
    CONSTRAINT chk_api_token_default_kind
        CHECK (is_default = (token_kind = 'default_intake')),
    CONSTRAINT chk_api_token_plaintext_envelope
        CHECK (
            (token_kind IN ('personal', 'service_account')
                AND plaintext_sealed IS NULL AND plaintext_nonce IS NULL)
            OR (token_kind IN ('default_intake', 'rum_client')
                AND plaintext_sealed IS NOT NULL AND plaintext_nonce IS NOT NULL)
        )
);
CREATE UNIQUE INDEX IF NOT EXISTS uniq_api_tokens_prefix ON api_tokens(prefix);
CREATE INDEX IF NOT EXISTS idx_api_token_org
    ON api_tokens(org_id, created_at_micros DESC);
CREATE INDEX IF NOT EXISTS idx_api_token_user ON api_tokens(user_id);
CREATE INDEX IF NOT EXISTS idx_api_token_service_account
    ON api_tokens(org_id, service_account_id, created_at_micros DESC)
    WHERE service_account_id IS NOT NULL;
-- 每个 (org, user) 至多一个「活的」默认 token；revoked 的旧默认不占名额。
CREATE UNIQUE INDEX IF NOT EXISTS uniq_api_tokens_default
    ON api_tokens(org_id, user_id) WHERE is_default AND NOT revoked;
CREATE UNIQUE INDEX IF NOT EXISTS uniq_api_tokens_rum_client
    ON api_tokens(org_id, application_id)
    WHERE token_kind = 'rum_client' AND NOT revoked;

CREATE TABLE IF NOT EXISTS audit_events (
    id            VARCHAR(64)  PRIMARY KEY,
    org_id        VARCHAR(64)  NOT NULL,
    actor_kind    VARCHAR(16)  NOT NULL,
    actor_id      VARCHAR(64)  NOT NULL,
    action        VARCHAR(64)  NOT NULL,
    target_kind   VARCHAR(64),
    target_id     VARCHAR(64),
    ip            VARCHAR(64),
    user_agent    TEXT,
    payload       JSONB        NOT NULL DEFAULT '{}'::JSONB,
    ts_micros     BIGINT       NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_audit_org_ts ON audit_events(org_id, ts_micros DESC);

CREATE TABLE IF NOT EXISTS quotas (
    org_id              VARCHAR(64)  PRIMARY KEY,
    max_intake_qps      INTEGER      NOT NULL DEFAULT 0,
    max_query_qps       INTEGER      NOT NULL DEFAULT 0,
    max_storage_bytes   BIGINT       NOT NULL DEFAULT 0,
    max_streams         INTEGER      NOT NULL DEFAULT 0,
    updated_at_micros   BIGINT       NOT NULL
);

CREATE TABLE IF NOT EXISTS license_usage_daily (
    day                 VARCHAR(10)  NOT NULL,    -- 'YYYY-MM-DD'
    org_id              VARCHAR(64)  NOT NULL,
    intake_bytes        BIGINT       NOT NULL DEFAULT 0,
    user_count          INTEGER      NOT NULL DEFAULT 0,
    PRIMARY KEY (day, org_id)
);

CREATE TABLE IF NOT EXISTS license_features (
    name                VARCHAR(64) PRIMARY KEY,
    enabled             BOOLEAN     NOT NULL DEFAULT FALSE,
    expires_at_micros   BIGINT,
    notes               TEXT
);

CREATE TABLE IF NOT EXISTS signing_secrets (
    id                  VARCHAR(64)  PRIMARY KEY,
    kind                VARCHAR(16)  NOT NULL,    -- 'jwt' | 'cookie' | ...
    secret              BYTEA        NOT NULL,
    is_primary          BOOLEAN      NOT NULL DEFAULT FALSE,
    created_at_micros   BIGINT       NOT NULL,
    retired_at_micros   BIGINT
);
-- 每个 kind 仅一行 is_primary=TRUE
CREATE UNIQUE INDEX IF NOT EXISTS uq_signing_primary
    ON signing_secrets(kind) WHERE is_primary;
CREATE INDEX IF NOT EXISTS idx_signing_active
    ON signing_secrets(kind, retired_at_micros);

CREATE TABLE IF NOT EXISTS rbac_policies (
    id                VARCHAR(64)  PRIMARY KEY,
    org_id            VARCHAR(64)  NOT NULL,
    subject_kind      VARCHAR(16)  NOT NULL,    -- user | team | role
    subject_id        VARCHAR(64)  NOT NULL,
    action            VARCHAR(32)  NOT NULL,
    resource_kind     VARCHAR(32)  NOT NULL,
    resource_id       VARCHAR(255),
    effect            VARCHAR(8)   NOT NULL,    -- allow | deny
    created_by        VARCHAR(64)  NOT NULL,
    created_at_micros BIGINT       NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_rbac_org_subject
    ON rbac_policies(org_id, subject_kind, subject_id);
CREATE INDEX IF NOT EXISTS idx_rbac_resource
    ON rbac_policies(org_id, resource_kind, resource_id);

-- ============================================================
-- Logical streams & unified file catalog
-- ============================================================

-- Stream retention 可选；settings 始终以 JSON object 作为默认值。
-- 逻辑流：用户可见的数据流实体。stream_type 是实体属性而非身份的一部分；
-- 身份只由 (org_id, id) 决定。stream_type 持久化 namespaced StreamTypeId；
-- 新类型由 StreamTypeRegistry 注册，不需要修改数据库约束。
CREATE TABLE IF NOT EXISTS logical_streams (
    id                  VARCHAR(64) PRIMARY KEY,
    org_id              VARCHAR(64)  NOT NULL,
    name                VARCHAR(255) NOT NULL,
    stream_type         VARCHAR(96)  NOT NULL,
    stream_type_version INTEGER      NOT NULL DEFAULT 1,
    schema              JSONB        NOT NULL,
    retention           JSONB,
    settings            JSONB        NOT NULL DEFAULT '{}'::JSONB,
    state               VARCHAR(16)  NOT NULL DEFAULT 'active',
    system              BOOLEAN      NOT NULL DEFAULT FALSE,
    created_at_micros   BIGINT       NOT NULL,
    updated_at_micros   BIGINT       NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS uniq_logical_streams_org_type_name
    ON logical_streams(org_id, stream_type, name);
-- 复合外键目标：让子表以 (org_id, id) 引用，保证跨租户记录无法关联。
CREATE UNIQUE INDEX IF NOT EXISTS uniq_logical_streams_org_id
    ON logical_streams(org_id, id);

-- ============================================================
-- 统一 FileCatalog：LogicalStream → PhysicalDataset → DataSegment
-- → Artifact → StoredObject。下面这组表是文件元数据的唯一事实来源；
-- Catalog transaction and immutable manifest generations are the only publication mechanism.
-- ============================================================

-- 逻辑流下的物理数据集。底层子系统只认 dataset id，不理解信号语义。
CREATE TABLE IF NOT EXISTS physical_datasets (
    org_id               VARCHAR(64) NOT NULL,
    id                   VARCHAR(64) NOT NULL,
    logical_stream_id    VARCHAR(64) NOT NULL,
    dataset_type         VARCHAR(96) NOT NULL,
    dataset_type_version INTEGER     NOT NULL DEFAULT 1,
    partition_policy     JSONB       NOT NULL DEFAULT '{}'::JSONB,
    storage_policy       JSONB       NOT NULL DEFAULT '{}'::JSONB,
    index_policy         JSONB       NOT NULL DEFAULT '{}'::JSONB,
    catalog_version      BIGINT      NOT NULL DEFAULT 0,
    state                VARCHAR(16) NOT NULL DEFAULT 'active',
    created_at_micros    BIGINT      NOT NULL,
    updated_at_micros    BIGINT      NOT NULL,
    PRIMARY KEY (org_id, id),
    CONSTRAINT fk_physical_datasets_stream
        FOREIGN KEY (org_id, logical_stream_id)
        REFERENCES logical_streams (org_id, id)
        ON DELETE CASCADE,
    CONSTRAINT uniq_physical_datasets_dataset_type
        UNIQUE (org_id, logical_stream_id, dataset_type)
);

-- 一次 flush / compaction 形成的不可变数据单元。
CREATE TABLE IF NOT EXISTS data_segments (
    org_id                 VARCHAR(64) NOT NULL,
    id                     VARCHAR(64) NOT NULL,
    dataset_id             VARCHAR(64) NOT NULL,
    partition_start_micros BIGINT      NOT NULL,
    partition_end_micros   BIGINT      NOT NULL,
    partition_shard        SMALLINT    NOT NULL DEFAULT 0,
    min_event_micros       BIGINT      NOT NULL,
    max_event_micros       BIGINT      NOT NULL,
    row_count              BIGINT      NOT NULL,
    schema_fingerprint     BIGINT,
    -- 列级 min/max（{"min":{},"max":{}}），查询侧保守跳段用。
    column_stats           JSONB       NOT NULL DEFAULT '{}'::JSONB,
    flush_id               TEXT,
    sequence_start         BIGINT,
    sequence_end           BIGINT,
    output_ordinal         INTEGER     NOT NULL DEFAULT 0,
    state                  VARCHAR(16) NOT NULL,
    visible_from_version   BIGINT      NOT NULL,
    retired_at_version     BIGINT,
    created_at_micros      BIGINT      NOT NULL,
    PRIMARY KEY (org_id, id),
    CONSTRAINT fk_data_segments_dataset
        FOREIGN KEY (org_id, dataset_id)
        REFERENCES physical_datasets (org_id, id)
        ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_data_segments_scan
    ON data_segments(org_id, dataset_id, state, partition_start_micros);
CREATE INDEX IF NOT EXISTS idx_data_segments_time
    ON data_segments(org_id, dataset_id, min_event_micros, max_event_micros)
    WHERE state = 'active';
CREATE INDEX IF NOT EXISTS idx_data_segments_flush
    ON data_segments(org_id, dataset_id, flush_id);

-- Segment 下的物理文件（Parquet 主数据、索引、统计、字典）。
-- 格式判定只看 artifact_type + format_version，不看对象 key 后缀。
CREATE TABLE IF NOT EXISTS artifacts (
    org_id             VARCHAR(64) NOT NULL,
    id                 VARCHAR(64) NOT NULL,
    segment_id         VARCHAR(64) NOT NULL,
    role               VARCHAR(16) NOT NULL,
    artifact_type      VARCHAR(96) NOT NULL,
    format_version     INTEGER     NOT NULL DEFAULT 1,
    object_key         TEXT        NOT NULL,
    size_bytes         BIGINT      NOT NULL CHECK (size_bytes >= 0),
    checksum           TEXT        NOT NULL,
    etag               TEXT,
    source_artifact_id VARCHAR(64),
    source_checksum    TEXT,
    schema_fingerprint BIGINT,
    state              VARCHAR(16) NOT NULL,
    failure_reason     TEXT,
    created_at_micros  BIGINT      NOT NULL,
    updated_at_micros  BIGINT      NOT NULL,
    PRIMARY KEY (org_id, id),
    CONSTRAINT fk_artifacts_segment
        FOREIGN KEY (org_id, segment_id)
        REFERENCES data_segments (org_id, id)
        ON DELETE CASCADE
);
CREATE UNIQUE INDEX IF NOT EXISTS uniq_artifacts_object_key
    ON artifacts(org_id, object_key);
-- 同一槽位只允许一个存活 Artifact；tombstoned 行等待延迟 GC 期间不占槽位，
-- 这样 index rebuild 才能先插新行再回收旧行。
CREATE UNIQUE INDEX IF NOT EXISTS uniq_artifacts_live_slot
    ON artifacts(org_id, segment_id, role, artifact_type)
    WHERE state IN ('pending', 'ready');
CREATE INDEX IF NOT EXISTS idx_artifacts_rebuild_scan
    ON artifacts(org_id, state)
    WHERE state IN ('pending', 'failed');

-- flush 幂等提交记录。唯一约束必须含 writer_node_id：多写入节点的
-- (epoch, sequence) 序号空间彼此独立，缺了节点维度会把并发 flush 误判为重试。
CREATE TABLE IF NOT EXISTS storage_flush_commits (
    org_id             VARCHAR(64)  NOT NULL,
    dataset_id         VARCHAR(64)  NOT NULL,
    flush_id           TEXT         NOT NULL,
    writer_node_id     VARCHAR(128) NOT NULL,
    writer_epoch       BIGINT       NOT NULL,
    sequence_start     BIGINT       NOT NULL,
    sequence_end       BIGINT       NOT NULL,
    committed_at_micros BIGINT      NOT NULL,
    PRIMARY KEY (org_id, dataset_id, flush_id),
    CONSTRAINT uniq_storage_flush_commits_range
        UNIQUE (org_id, dataset_id, writer_node_id, writer_epoch,
                sequence_start, sequence_end),
    CONSTRAINT fk_storage_flush_commits_dataset
        FOREIGN KEY (org_id, dataset_id)
        REFERENCES physical_datasets (org_id, id)
        ON DELETE CASCADE
);

-- 每个 (dataset, node, epoch) 已提交进 Catalog 的最高 WAL 序号。
-- 恢复时据此跳过已提交记录；查询侧据此对 Buffer 去重。
CREATE TABLE IF NOT EXISTS wal_checkpoints (
    org_id             VARCHAR(64)  NOT NULL,
    dataset_id         VARCHAR(64)  NOT NULL,
    writer_node_id     VARCHAR(128) NOT NULL,
    writer_epoch       BIGINT       NOT NULL,
    committed_sequence BIGINT       NOT NULL,
    updated_at_micros  BIGINT       NOT NULL,
    PRIMARY KEY (org_id, dataset_id, writer_node_id, writer_epoch),
    CONSTRAINT fk_wal_checkpoints_dataset
        FOREIGN KEY (org_id, dataset_id)
        REFERENCES physical_datasets (org_id, id)
        ON DELETE CASCADE
);

-- 封存冷分区的不可变 Manifest 指针（Manifest 本体在 object store）。
CREATE TABLE IF NOT EXISTS partition_manifests (
    org_id                 VARCHAR(64) NOT NULL,
    dataset_id             VARCHAR(64) NOT NULL,
    partition_start_micros BIGINT      NOT NULL,
    partition_end_micros   BIGINT      NOT NULL,
    partition_shard        SMALLINT    NOT NULL DEFAULT 0,
    generation             BIGINT      NOT NULL,
    object_key             TEXT        NOT NULL,
    size_bytes             BIGINT      NOT NULL,
    checksum               TEXT        NOT NULL,
    etag                   TEXT,
    segment_count          INTEGER     NOT NULL,
    state                  VARCHAR(16) NOT NULL,
    created_at_micros      BIGINT      NOT NULL,
    PRIMARY KEY (org_id, dataset_id, partition_start_micros, partition_shard, generation),
    CONSTRAINT fk_partition_manifests_dataset
        FOREIGN KEY (org_id, dataset_id)
        REFERENCES physical_datasets (org_id, id)
        ON DELETE CASCADE
);
CREATE UNIQUE INDEX IF NOT EXISTS uniq_partition_manifests_object_key
    ON partition_manifests(org_id, object_key);
CREATE UNIQUE INDEX IF NOT EXISTS uniq_partition_manifests_active
    ON partition_manifests(org_id, dataset_id, partition_start_micros, partition_shard)
    WHERE state = 'active';

-- 延迟对象 GC 队列。所有对象删除都必须经 tombstone → 到期 → 二次确认。
CREATE TABLE IF NOT EXISTS object_gc_queue (
    org_id            VARCHAR(64) NOT NULL,
    object_key        TEXT        NOT NULL,
    checksum          TEXT,
    reason            VARCHAR(32) NOT NULL,
    not_before_micros BIGINT      NOT NULL,
    attempt_count     INTEGER     NOT NULL DEFAULT 0,
    state             VARCHAR(16) NOT NULL DEFAULT 'pending',
    last_error        TEXT,
    created_at_micros BIGINT      NOT NULL,
    updated_at_micros BIGINT      NOT NULL,
    PRIMARY KEY (org_id, object_key)
);
CREATE INDEX IF NOT EXISTS idx_object_gc_queue_due
    ON object_gc_queue(state, not_before_micros);
CREATE INDEX IF NOT EXISTS idx_artifacts_rebuild
    ON artifacts(org_id, state, updated_at_micros)
    WHERE role = 'index' AND state IN ('pending', 'failed');

CREATE TABLE IF NOT EXISTS file_download_tokens (
    token             VARCHAR(64)  PRIMARY KEY,
    org_id            VARCHAR(64)  NOT NULL,
    user_id           VARCHAR(64)  NOT NULL,
    object_keys_json  JSONB        NOT NULL,
    expires_at_micros BIGINT       NOT NULL,
    created_at_micros BIGINT       NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_file_token_expires
    ON file_download_tokens(expires_at_micros);

-- ============================================================
-- Dashboards / saved views / annotations / debug artifacts / short URLs
-- ============================================================

CREATE TABLE IF NOT EXISTS folders (
    id          VARCHAR(64) PRIMARY KEY,
    org_id      VARCHAR(64)  NOT NULL,
    name        VARCHAR(255) NOT NULL,
    parent_id   VARCHAR(64)
);

CREATE TABLE IF NOT EXISTS dashboards (
    id                  VARCHAR(64) PRIMARY KEY,
    org_id              VARCHAR(64)  NOT NULL,
    folder_id           VARCHAR(64),
    uid                 VARCHAR(64)  NOT NULL,
    title               VARCHAR(255) NOT NULL,
    tags                JSONB        NOT NULL,
    model               JSONB        NOT NULL,
    version             INTEGER      NOT NULL,
    created_at_micros   BIGINT       NOT NULL,
    updated_at_micros   BIGINT       NOT NULL,
    created_by          VARCHAR(64)  NOT NULL,
    updated_by          VARCHAR(64)  NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS uniq_dashboards_org_uid
    ON dashboards(org_id, uid);

CREATE TABLE IF NOT EXISTS saved_views (
    id                 VARCHAR(64)  PRIMARY KEY,
    org_id             VARCHAR(64)  NOT NULL,
    owner_user_id      VARCHAR(64)  NOT NULL,
    name               VARCHAR(255) NOT NULL,
    language           VARCHAR(16)  NOT NULL,
    statement          TEXT         NOT NULL,
    time_range_secs    INTEGER      NOT NULL,
    stream             VARCHAR(255),
    tags               JSONB        NOT NULL DEFAULT '[]'::JSONB,
    pinned             BOOLEAN      NOT NULL DEFAULT FALSE,
    created_at_micros  BIGINT       NOT NULL,
    updated_at_micros  BIGINT       NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_saved_views_org_owner
    ON saved_views(org_id, owner_user_id);

CREATE TABLE IF NOT EXISTS annotations (
    id                  VARCHAR(64)  PRIMARY KEY,
    org_id              VARCHAR(64)  NOT NULL,
    title               TEXT         NOT NULL,
    description         TEXT,
    tags                JSONB        NOT NULL DEFAULT '[]',
    time_start_micros   BIGINT       NOT NULL,
    time_end_micros     BIGINT       NOT NULL,
    dashboard_id        VARCHAR(64),
    stream_name         VARCHAR(255),
    created_by          VARCHAR(64)  NOT NULL,
    created_at_micros   BIGINT       NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_annotation_org_time
    ON annotations(org_id, time_start_micros DESC);
CREATE INDEX IF NOT EXISTS idx_annotation_dashboard
    ON annotations(dashboard_id);

CREATE TABLE IF NOT EXISTS debug_artifacts (
    id                 VARCHAR(64)  PRIMARY KEY,
    org_id             VARCHAR(64)  NOT NULL,
    application_id     VARCHAR(128) NOT NULL DEFAULT '',
    service            VARCHAR(255) NOT NULL,
    release            VARCHAR(64)  NOT NULL,
    artifact_kind      VARCHAR(32)  NOT NULL DEFAULT 'javascript_sourcemap',
    platform           VARCHAR(16)  NOT NULL DEFAULT 'web',
    architecture       VARCHAR(32)  NOT NULL DEFAULT '',
    debug_id           VARCHAR(128) NOT NULL DEFAULT '',
    filename           VARCHAR(255) NOT NULL,
    object_key         TEXT         NOT NULL,
    size_bytes         BIGINT       NOT NULL DEFAULT 0,
    checksum_sha256    VARCHAR(64)  NOT NULL DEFAULT '',
    uploaded_at_micros BIGINT       NOT NULL,
    CONSTRAINT chk_debug_artifact_application
        CHECK (application_id ~ '^[A-Za-z0-9._:-]{1,128}$'),
    CONSTRAINT chk_debug_artifact_kind CHECK (
        artifact_kind IN (
            'javascript_sourcemap',
            'flutter_symbols',
            'android_mapping',
            'android_native_symbols',
            'apple_dsym'
        )
    )
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_debug_artifact_identity
    ON debug_artifacts(
        org_id,
        application_id,
        service,
        release,
        artifact_kind,
        platform,
        architecture,
        debug_id,
        filename
    );
CREATE INDEX IF NOT EXISTS idx_debug_artifact_symbolication
    ON debug_artifacts(org_id, application_id, service, release, artifact_kind, platform);

CREATE TABLE IF NOT EXISTS investigation_blobs (
    id                 VARCHAR(64)  PRIMARY KEY,
    org_id             VARCHAR(64)  NOT NULL,
    -- 内容外置到对象存储：新行只留 object_key 指针，payload 可空以读旧行（双读迁移）。
    payload            JSONB,
    object_key         TEXT,
    created_at_micros  BIGINT       NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_investigation_blobs_created
    ON investigation_blobs(created_at_micros);
CREATE INDEX IF NOT EXISTS idx_investigation_blobs_org_id
    ON investigation_blobs(org_id, id);

-- ============================================================
-- Alerting (rules / incidents / schedules / escalation / channels / deliveries)
-- ============================================================

-- Alert rule 同时支持定时与异常检测，并可冻结通知模板内容。
CREATE TABLE IF NOT EXISTS alert_rules (
    id                      VARCHAR(64) PRIMARY KEY,
    org_id                  VARCHAR(64)  NOT NULL,
    name                    VARCHAR(255) NOT NULL,
    description             TEXT         NOT NULL,
    enabled                 BOOLEAN      NOT NULL,
    query                   JSONB        NOT NULL,
    trigger                 JSONB        NOT NULL,
    escalation_policy_id    VARCHAR(64)  NOT NULL,
    labels                  JSONB        NOT NULL,
    annotations             JSONB        NOT NULL,
    last_eval_at_micros     BIGINT,
    last_state              JSONB        NOT NULL,
    kind                    VARCHAR(16)  NOT NULL DEFAULT 'scheduled',
    anomaly_params_json     JSONB,
    body_template           TEXT,
    -- 多档严重度阈值：[{severity, operator, threshold, for_periods}]，空数组 = 走单档 trigger（历史行为）。
    thresholds              JSONB        NOT NULL DEFAULT '[]'::jsonb,
    -- 单档规则的显式兜底严重度（critical|error|warning|info）；NULL = 由评估器推导。
    severity                VARCHAR(16),
    -- 关联的通知模板（alert_templates.id）；NULL = 用渠道默认格式。
    template_id             VARCHAR(64),
    created_at_micros       BIGINT       NOT NULL,
    updated_at_micros       BIGINT       NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_alert_rules_org_enabled
    ON alert_rules(org_id, enabled);

CREATE TABLE IF NOT EXISTS alert_rule_eval_state (
    rule_id                 VARCHAR(64) PRIMARY KEY,
    consecutive_matches     INTEGER     NOT NULL DEFAULT 0,
    last_eval_at_micros     BIGINT      NOT NULL,
    last_matched            BOOLEAN     NOT NULL DEFAULT FALSE,
    -- 每档连续命中计数 { severity: streak }，用于多档各自的 for_periods 去抖。
    severity_streaks        JSONB       NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE IF NOT EXISTS incidents (
    id                              VARCHAR(64) PRIMARY KEY,
    org_id                          VARCHAR(64)  NOT NULL,
    rule_id                         VARCHAR(64)  NOT NULL,
    escalation_policy_id            VARCHAR(64)  NOT NULL,
    status                          VARCHAR(16)  NOT NULL,
    severity                        VARCHAR(16)  NOT NULL,
    summary                         TEXT         NOT NULL,
    fingerprint                     VARCHAR(128) NOT NULL,
    current_step                    INTEGER      NOT NULL,
    -- 升级策略已循环遍数（repeat/max_loops），从 0 起。
    current_loop                    INTEGER      NOT NULL DEFAULT 0,
    current_step_started_at_micros  BIGINT       NOT NULL,
    assignees                       JSONB        NOT NULL,
    -- 触发时 freeze 的 label / annotation 快照，drawer 用来展示 service / env / runbook 链接等。
    labels                          JSONB        NOT NULL DEFAULT '{}'::jsonb,
    annotations                     JSONB        NOT NULL DEFAULT '{}'::jsonb,
    -- 跨信号 handle：从触发查询里采样出的 trace_id / host / service 列表（保留顺序去重）。
    trace_ids                       JSONB        NOT NULL DEFAULT '[]'::jsonb,
    host_ids                        JSONB        NOT NULL DEFAULT '[]'::jsonb,
    affected_services               JSONB        NOT NULL DEFAULT '[]'::jsonb,
    -- 触发查询的元数据（语言、原始 statement、sample 行）。仅 detail endpoint 返回 sample_values。
    triggering_query                JSONB,
    created_at_micros               BIGINT       NOT NULL,
    acknowledged_at_micros          BIGINT,
    acknowledged_by                 VARCHAR(64),
    resolved_at_micros              BIGINT,
    resolved_by                     VARCHAR(64),
    -- 触发时 freeze 的通知模板正文（来自规则关联模板）；NULL = 用渠道默认格式。
    body_template                   TEXT
);
CREATE UNIQUE INDEX IF NOT EXISTS uniq_incidents_fingerprint
    ON incidents(org_id, fingerprint)
    WHERE status IN ('open', 'acknowledged');
CREATE INDEX IF NOT EXISTS idx_incidents_fingerprint_history
    ON incidents(org_id, fingerprint, created_at_micros DESC);
CREATE INDEX IF NOT EXISTS idx_incidents_status
    ON incidents(org_id, status);

CREATE TABLE IF NOT EXISTS incident_groups (
    id                  VARCHAR(64)  PRIMARY KEY,
    org_id              VARCHAR(64)  NOT NULL,
    alert_rule_id       VARCHAR(64)  NOT NULL,
    -- group_by 派生的 fingerprint（如 "service=api;region=us"）可能较长，用 TEXT。
    fingerprint         TEXT         NOT NULL,
    state               VARCHAR(16)  NOT NULL DEFAULT 'open',    -- open | acked | resolved
    incident_count      INTEGER      NOT NULL DEFAULT 1,
    first_at_micros     BIGINT       NOT NULL,
    last_at_micros      BIGINT       NOT NULL,
    acked_by            VARCHAR(64),
    acked_at_micros     BIGINT,
    resolved_at_micros  BIGINT
);
CREATE INDEX IF NOT EXISTS idx_incident_group_rule_fp
    ON incident_groups(alert_rule_id, fingerprint, last_at_micros DESC);
CREATE INDEX IF NOT EXISTS idx_incident_group_org_state
    ON incident_groups(org_id, state, last_at_micros DESC);

CREATE TABLE IF NOT EXISTS schedules (
    id                  VARCHAR(64) PRIMARY KEY,
    org_id              VARCHAR(64)  NOT NULL,
    name                VARCHAR(255) NOT NULL,
    timezone            VARCHAR(64)  NOT NULL,
    rotations           JSONB        NOT NULL,
    overrides           JSONB        NOT NULL,
    created_at_micros   BIGINT       NOT NULL,
    updated_at_micros   BIGINT       NOT NULL
);

CREATE TABLE IF NOT EXISTS escalation_policies (
    id          VARCHAR(64) PRIMARY KEY,
    org_id      VARCHAR(64)  NOT NULL,
    name        VARCHAR(255) NOT NULL,
    steps       JSONB        NOT NULL,
    "repeat"    BOOLEAN      NOT NULL,
    max_loops   INTEGER      NOT NULL
);

-- Alert channel 可覆盖规则级通知模板。
CREATE TABLE IF NOT EXISTS notify_channels (
    id              VARCHAR(64) PRIMARY KEY,
    org_id          VARCHAR(64)  NOT NULL,
    name            VARCHAR(255) NOT NULL,
    kind            JSONB        NOT NULL,
    enabled         BOOLEAN      NOT NULL,
    body_template   TEXT,
    -- 渠道级通知模板（alert_templates.id）；优先于规则级，NULL = 用规则级或默认格式。
    template_id     VARCHAR(64)
);

CREATE TABLE IF NOT EXISTS deliveries (
    id                  VARCHAR(64) PRIMARY KEY,
    incident_id         VARCHAR(64) NOT NULL,
    channel_id          VARCHAR(64) NOT NULL,
    target_user_id      VARCHAR(64),
    status              VARCHAR(16) NOT NULL,
    attempted_at_micros BIGINT      NOT NULL,
    finished_at_micros  BIGINT,
    error               TEXT
);
CREATE INDEX IF NOT EXISTS idx_deliveries_incident
    ON deliveries(incident_id);

CREATE TABLE IF NOT EXISTS alert_templates (
    id                VARCHAR(64)  PRIMARY KEY,
    org_id            VARCHAR(64)  NOT NULL,
    name              VARCHAR(255) NOT NULL,
    body              TEXT         NOT NULL,
    format            VARCHAR(16)  NOT NULL DEFAULT 'text',
    created_at_micros BIGINT       NOT NULL,
    updated_at_micros BIGINT       NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_alert_template_org_name
    ON alert_templates(org_id, name);

CREATE TABLE IF NOT EXISTS regex_patterns (
    id                VARCHAR(64)  PRIMARY KEY,
    org_id            VARCHAR(64)  NOT NULL,
    name              VARCHAR(255) NOT NULL,
    pattern           TEXT         NOT NULL,
    description       TEXT         NOT NULL DEFAULT '',
    -- 命中片段替换串（支持 $1 捕获组回引）。
    replacement       TEXT         NOT NULL DEFAULT '[REDACTED]',
    -- 写入前对所有字符串值做不可逆脱敏（off 时仅查询端 mask(col) 可用）。
    apply_on_intake   BOOLEAN      NOT NULL DEFAULT false,
    created_at_micros BIGINT       NOT NULL,
    updated_at_micros BIGINT       NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_regex_pattern_org_name
    ON regex_patterns(org_id, name);

-- Organization-level dynamic field masking rules. Raw values remain stored and
-- query execution uses raw data; masking is applied at the response boundary.
CREATE TABLE IF NOT EXISTS field_masking_rules (
    id                VARCHAR(64)  PRIMARY KEY,
    org_id            VARCHAR(64)  NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name              VARCHAR(255) NOT NULL,
    priority          INTEGER      NOT NULL,
    enabled           BOOLEAN      NOT NULL DEFAULT true,
    field_pattern     VARCHAR(255) NOT NULL,
    stream_pattern    VARCHAR(255),
    stream_type       VARCHAR(96),
    algorithm         JSONB        NOT NULL,
    created_at_micros BIGINT       NOT NULL,
    updated_at_micros BIGINT       NOT NULL,
    CONSTRAINT chk_field_masking_priority CHECK (priority >= 0),
    CONSTRAINT chk_field_masking_stream_type
        CHECK (stream_type IS NULL OR stream_type ~ '^[a-z][a-z0-9_-]*(\.[a-z][a-z0-9_-]*)*$')
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_field_masking_rule_org_name
    ON field_masking_rules(org_id, name);
CREATE UNIQUE INDEX IF NOT EXISTS uq_field_masking_rule_org_priority
    ON field_masking_rules(org_id, priority);

-- ============================================================
-- Functions / pipelines / scheduled pipelines / pipeline runs
-- ============================================================

CREATE TABLE IF NOT EXISTS functions (
    id                 VARCHAR(64)  PRIMARY KEY,
    org_id             VARCHAR(64)  NOT NULL,
    name               VARCHAR(255) NOT NULL,
    language           VARCHAR(16)  NOT NULL,    -- 'vrl' | 'js'
    source             TEXT         NOT NULL,
    params_schema      JSONB        NOT NULL DEFAULT '{}'::JSONB,
    created_at_micros  BIGINT       NOT NULL,
    updated_at_micros  BIGINT       NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS uniq_functions_org_name
    ON functions(org_id, name);

CREATE TABLE IF NOT EXISTS pipelines (
    id                    VARCHAR(64)  PRIMARY KEY,
    org_id                VARCHAR(64)  NOT NULL,
    name                  VARCHAR(255) NOT NULL,
    stream_target_hash    VARCHAR(64)  NOT NULL,
    steps                 JSONB        NOT NULL,
    enabled               BOOLEAN      NOT NULL DEFAULT TRUE,
    created_at_micros     BIGINT       NOT NULL,
    updated_at_micros     BIGINT       NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS uniq_pipelines_org_name
    ON pipelines(org_id, name);
-- 同 stream + enabled=true 唯一
CREATE UNIQUE INDEX IF NOT EXISTS uniq_pipelines_target_enabled
    ON pipelines(stream_target_hash) WHERE enabled;

CREATE TABLE IF NOT EXISTS scheduled_pipelines (
    id                 VARCHAR(64)  PRIMARY KEY,
    org_id             VARCHAR(64)  NOT NULL,
    name               VARCHAR(255) NOT NULL,
    source_stream      VARCHAR(255) NOT NULL,
    target_stream      VARCHAR(255) NOT NULL,
    function_steps     JSONB        NOT NULL,
    cron               VARCHAR(64)  NOT NULL,
    lookback_secs      INTEGER      NOT NULL DEFAULT 300,
    last_run_at_micros BIGINT,
    enabled            BOOLEAN      NOT NULL DEFAULT TRUE,
    created_at_micros  BIGINT       NOT NULL,
    updated_at_micros  BIGINT       NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_scheduled_pipeline_org_name
    ON scheduled_pipelines(org_id, name);

CREATE TABLE IF NOT EXISTS pipeline_runs (
    id                  VARCHAR(64)  PRIMARY KEY,
    pipeline_id         VARCHAR(64)  NOT NULL,
    org_id              VARCHAR(64)  NOT NULL,
    state               VARCHAR(16)  NOT NULL,
    started_at_micros   BIGINT       NOT NULL,
    finished_at_micros  BIGINT,
    scanned_rows        BIGINT       NOT NULL DEFAULT 0,
    error               TEXT
);
CREATE INDEX IF NOT EXISTS idx_pipeline_runs_pipeline_started
    ON pipeline_runs(pipeline_id, started_at_micros DESC);

CREATE TABLE IF NOT EXISTS extend_kv (
    id                 VARCHAR(64)  PRIMARY KEY,
    org_id             VARCHAR(64)  NOT NULL,
    table_name         VARCHAR(255) NOT NULL,
    key                VARCHAR(512) NOT NULL,
    value_json         JSONB        NOT NULL,
    updated_at_micros  BIGINT       NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_extend_org_table_key
    ON extend_kv(org_id, table_name, key);

-- ============================================================
-- Search jobs / reports / log patterns
-- ============================================================

CREATE TABLE IF NOT EXISTS search_jobs (
    id                  VARCHAR(64)  PRIMARY KEY,
    org_id              VARCHAR(64)  NOT NULL,
    user_id             VARCHAR(64)  NOT NULL,
    role_key            VARCHAR(64)  NOT NULL DEFAULT '',
    request_json        JSONB        NOT NULL,
    state               VARCHAR(16)  NOT NULL DEFAULT 'pending',  -- pending | running | done | failed | cancelled
    attempt             INTEGER      NOT NULL DEFAULT 0,
    result_object_key   TEXT,
    result_rows         BIGINT,
    error               TEXT,
    submitted_at_micros BIGINT       NOT NULL,
    started_at_micros   BIGINT,
    finished_at_micros  BIGINT,
    expires_at_micros   BIGINT       NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_search_job_state
    ON search_jobs(state, submitted_at_micros);
CREATE INDEX IF NOT EXISTS idx_search_job_org
    ON search_jobs(org_id, submitted_at_micros DESC);
CREATE INDEX IF NOT EXISTS idx_search_job_expires
    ON search_jobs(expires_at_micros);

CREATE TABLE IF NOT EXISTS scheduled_reports (
    id                  VARCHAR(64)  PRIMARY KEY,
    org_id              VARCHAR(64)  NOT NULL,
    name                VARCHAR(255) NOT NULL,
    dashboard_id        VARCHAR(64),
    saved_view_id       VARCHAR(64),
    cron                VARCHAR(64)  NOT NULL,
    recipients_json     JSONB        NOT NULL,
    format              VARCHAR(16)  NOT NULL,    -- png | pdf | csv | svg | json
    time_range_json     JSONB        NOT NULL DEFAULT '{}',
    enabled             BOOLEAN      NOT NULL DEFAULT TRUE,
    last_run_at_micros  BIGINT,
    created_at_micros   BIGINT       NOT NULL,
    updated_at_micros   BIGINT       NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_report_enabled
    ON scheduled_reports(enabled);

CREATE TABLE IF NOT EXISTS report_deliveries (
    id                  VARCHAR(64)  PRIMARY KEY,
    report_id           VARCHAR(64)  NOT NULL,
    org_id              VARCHAR(64)  NOT NULL,
    status              VARCHAR(16)  NOT NULL,    -- pending | sent | failed
    attempt             INTEGER      NOT NULL DEFAULT 1,
    recipient_kind      VARCHAR(16)  NOT NULL,
    recipient_target    TEXT         NOT NULL,
    error               TEXT,
    attempted_at_micros BIGINT       NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_delivery_report
    ON report_deliveries(report_id, attempted_at_micros DESC);

CREATE TABLE IF NOT EXISTS log_patterns (
    id                VARCHAR(64)  PRIMARY KEY,
    org_id            VARCHAR(64)  NOT NULL,
    name              VARCHAR(255) NOT NULL,
    regex             TEXT         NOT NULL,
    capture_groups    JSONB        NOT NULL DEFAULT '[]',
    category          VARCHAR(64)  NOT NULL,
    priority          INTEGER      NOT NULL DEFAULT 0,
    stream_filter     VARCHAR(255),
    created_at_micros BIGINT       NOT NULL,
    updated_at_micros BIGINT       NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_log_pattern_org_name
    ON log_patterns(org_id, name);
CREATE INDEX IF NOT EXISTS idx_log_pattern_priority
    ON log_patterns(org_id, priority DESC);

-- ============================================================
-- Observability extras: service graph / RUM replay
-- ============================================================

CREATE TABLE IF NOT EXISTS service_graph_edges (
    id                   VARCHAR(64)  PRIMARY KEY,
    org_id               VARCHAR(64)  NOT NULL,
    client_service       VARCHAR(255) NOT NULL,
    server_service       VARCHAR(255) NOT NULL,
    bucket_at_micros     BIGINT       NOT NULL,
    request_count        BIGINT       NOT NULL DEFAULT 0,
    error_count          BIGINT       NOT NULL DEFAULT 0,
    p50_us               BIGINT,
    p95_us               BIGINT,
    p99_us               BIGINT
);
CREATE INDEX IF NOT EXISTS idx_sg_org_bucket
    ON service_graph_edges(org_id, bucket_at_micros DESC);

CREATE TABLE IF NOT EXISTS rum_replay_events (
    id                  VARCHAR(64)  PRIMARY KEY,
    org_id              VARCHAR(64)  NOT NULL,
    application_id      VARCHAR(128) NOT NULL DEFAULT '',
    session_id          VARCHAR(64)  NOT NULL,
    seq                 INTEGER      NOT NULL,
    -- v1 = zstd-compressed NDJSON; independent from the object-key layout version.
    format_version      INTEGER      NOT NULL DEFAULT 1 CHECK (format_version > 0),
    object_key          TEXT         NOT NULL,
    bytes_uncompressed  BIGINT       NOT NULL,
    event_count         INTEGER      NOT NULL,
    has_full_snapshot   BOOLEAN      NOT NULL DEFAULT FALSE,
    content_hash        VARCHAR(64)  NOT NULL,
    first_event_at_micros BIGINT     NOT NULL,
    created_at_micros   BIGINT       NOT NULL,
    UNIQUE (org_id, application_id, session_id, seq)
);
CREATE INDEX IF NOT EXISTS idx_rum_replay_session
    ON rum_replay_events(org_id, application_id, session_id, seq);
CREATE INDEX IF NOT EXISTS idx_rum_replay_retention
    ON rum_replay_events(created_at_micros);
CREATE INDEX IF NOT EXISTS idx_rum_replay_available
    ON rum_replay_events(org_id, first_event_at_micros DESC, session_id)
    WHERE has_full_snapshot = TRUE;

-- ============================================================
-- Cluster / cipher / connectors / remote clusters
-- ============================================================

CREATE TABLE IF NOT EXISTS cluster_nodes (
    node_id                     VARCHAR(64)  PRIMARY KEY,
    -- 多角色节点用逗号拼接的角色集（如 "intake,querier"），加宽以容纳多个角色。
    role                        VARCHAR(128) NOT NULL,
    advertise_addr              VARCHAR(255) NOT NULL,
    started_at_micros           BIGINT       NOT NULL,
    last_heartbeat_at_micros    BIGINT       NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_cluster_nodes_role
    ON cluster_nodes(role);
CREATE INDEX IF NOT EXISTS idx_cluster_nodes_last_heartbeat
    ON cluster_nodes(last_heartbeat_at_micros);

CREATE TABLE IF NOT EXISTS cipher_keys (
    id                 VARCHAR(64)  PRIMARY KEY,
    org_id             VARCHAR(64)  NOT NULL,
    name               VARCHAR(255) NOT NULL,
    alg                VARCHAR(16)  NOT NULL DEFAULT 'aes-256-gcm',
    version            INTEGER      NOT NULL DEFAULT 1,
    key_material_enc   BYTEA        NOT NULL,
    nonce              BYTEA        NOT NULL,
    created_at_micros  BIGINT       NOT NULL,
    rotated_at_micros  BIGINT
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_cipher_org_name_ver
    ON cipher_keys(org_id, name, version);

CREATE TABLE IF NOT EXISTS connectors (
    id                 VARCHAR(64)  PRIMARY KEY,
    org_id             VARCHAR(64)  NOT NULL,
    name               VARCHAR(255) NOT NULL,
    kind               VARCHAR(32)  NOT NULL,
    config_json        JSONB        NOT NULL,
    enabled            BOOLEAN      NOT NULL DEFAULT TRUE,
    last_run_at_micros BIGINT,
    created_at_micros  BIGINT       NOT NULL,
    updated_at_micros  BIGINT       NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_connector_org_name
    ON connectors(org_id, name);

CREATE TABLE IF NOT EXISTS remote_clusters (
    id                VARCHAR(64)  PRIMARY KEY,
    name              VARCHAR(255) NOT NULL UNIQUE,
    advertise_addr    VARCHAR(255) NOT NULL,
    token_secret_ref  VARCHAR(255) NOT NULL,
    tls_verify        BOOLEAN      NOT NULL DEFAULT TRUE,
    enabled           BOOLEAN      NOT NULL DEFAULT TRUE,
    -- gossip 发现来源标记：discovered 的集群默认 enabled=false，需 admin 手动启用 + 配 token/org_map。
    discovered        BOOLEAN      NOT NULL DEFAULT false,
    created_at_micros BIGINT       NOT NULL,
    updated_at_micros BIGINT       NOT NULL
);

-- ============================================================
-- : actions / copilot / marketplace / model pricing / domains / AI toolsets
-- ============================================================

CREATE TABLE IF NOT EXISTS actions (
    id                 VARCHAR(64)  PRIMARY KEY,
    org_id             VARCHAR(64)  NOT NULL,
    name               VARCHAR(255) NOT NULL,
    kind               JSONB        NOT NULL,    -- ActionKind 序列化（webhook | script）
    enabled            BOOLEAN      NOT NULL DEFAULT TRUE,
    created_at_micros  BIGINT       NOT NULL,
    updated_at_micros  BIGINT       NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS uniq_actions_org_name
    ON actions(org_id, name);

CREATE TABLE IF NOT EXISTS action_executions (
    id                  VARCHAR(64)  PRIMARY KEY,
    action_id           VARCHAR(64)  NOT NULL,
    org_id              VARCHAR(64)  NOT NULL,
    incident_id         VARCHAR(64),
    status              VARCHAR(16)  NOT NULL,    -- success | failed | timeout | skipped
    status_code         INTEGER,
    response_body       TEXT,
    error               TEXT,
    duration_ms         BIGINT       NOT NULL,
    executed_at_micros  BIGINT       NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_action_exec_action
    ON action_executions(action_id, executed_at_micros DESC);
CREATE INDEX IF NOT EXISTS idx_action_exec_org
    ON action_executions(org_id, executed_at_micros DESC);

CREATE TABLE IF NOT EXISTS chat_sessions (
    id                 VARCHAR(64)  PRIMARY KEY,
    org_id             VARCHAR(64)  NOT NULL,
    user_id            VARCHAR(64)  NOT NULL,
    provider           VARCHAR(32)  NOT NULL,    -- openai | anthropic | openai_compatible
    model              VARCHAR(64)  NOT NULL,
    title              VARCHAR(255) NOT NULL DEFAULT '',
    created_at_micros  BIGINT       NOT NULL,
    updated_at_micros  BIGINT       NOT NULL,
    -- AI anomaly/root-cause chat 扩展列（nullable，旧行兼容）。
    provider_id             VARCHAR(64),
    analysis_mode           VARCHAR(32),
    time_range_start_micros BIGINT,
    time_range_end_micros   BIGINT,
    archive_object_key      TEXT,
    deleted_at_micros       BIGINT
);
CREATE INDEX IF NOT EXISTS idx_chat_sessions_org_user
    ON chat_sessions(org_id, user_id, updated_at_micros DESC);
-- 软删过滤索引：正常 history 列表只看 deleted_at_micros IS NULL 的会话。
CREATE INDEX IF NOT EXISTS idx_chat_sessions_org_user_active
    ON chat_sessions(org_id, user_id, updated_at_micros DESC)
    WHERE deleted_at_micros IS NULL;

CREATE TABLE IF NOT EXISTS chat_messages (
    id                 VARCHAR(64)  PRIMARY KEY,
    session_id         VARCHAR(64)  NOT NULL,
    org_id             VARCHAR(64)  NOT NULL,
    role               VARCHAR(16)  NOT NULL,    -- system | user | assistant | tool
    content            TEXT         NOT NULL,
    tool_calls_json    JSONB,
    tool_result_for    VARCHAR(64),
    prompt_tokens      BIGINT,
    completion_tokens  BIGINT,
    cost_usd           DOUBLE PRECISION,
    created_at_micros  BIGINT       NOT NULL,
    -- AI chat prompt 溯源 + evidence 摘要（nullable，旧行兼容）。
    prompt_template_id   VARCHAR(64),
    prompt_builtin_key   VARCHAR(64),
    prompt_version       INTEGER,
    prompt_hash          VARCHAR(64),
    evidence_json        JSONB
);
CREATE INDEX IF NOT EXISTS idx_chat_messages_session
    ON chat_messages(session_id, created_at_micros);

CREATE TABLE IF NOT EXISTS marketplace_subscriptions (
    id                 VARCHAR(64)  PRIMARY KEY,
    org_id             VARCHAR(64)  NOT NULL,
    provider           VARCHAR(16)  NOT NULL,    -- aws | azure
    external_id        VARCHAR(255) NOT NULL,
    state              VARCHAR(16)  NOT NULL,    -- pending | active | suspended | cancelled
    plan_id            VARCHAR(128),
    metadata           JSONB        NOT NULL DEFAULT '{}'::JSONB,
    created_at_micros  BIGINT       NOT NULL,
    updated_at_micros  BIGINT       NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS uniq_marketplace_provider_external
    ON marketplace_subscriptions(provider, external_id);

CREATE TABLE IF NOT EXISTS model_prices (
    provider                VARCHAR(32) NOT NULL,
    model                   VARCHAR(64) NOT NULL,
    prompt_usd_per_1k       DOUBLE PRECISION NOT NULL,
    completion_usd_per_1k   DOUBLE PRECISION NOT NULL,
    updated_at_micros       BIGINT NOT NULL,
    PRIMARY KEY (provider, model)
);

CREATE TABLE IF NOT EXISTS domains (
    id                    VARCHAR(64)  PRIMARY KEY,
    org_id                VARCHAR(64)  NOT NULL,
    hostname              VARCHAR(255) NOT NULL,
    state                 VARCHAR(16)  NOT NULL,    -- pending | provisioning | active | failed | expired
    cert_pem              TEXT,
    cert_not_after_micros BIGINT,
    last_error            TEXT,
    created_at_micros     BIGINT       NOT NULL,
    updated_at_micros     BIGINT       NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS uniq_domains_hostname
    ON domains(hostname);
CREATE INDEX IF NOT EXISTS idx_domains_org
    ON domains(org_id, updated_at_micros DESC);

CREATE TABLE IF NOT EXISTS acme_challenges (
    token              VARCHAR(255) PRIMARY KEY,
    domain_id          VARCHAR(64)  NOT NULL,
    key_authorization  TEXT         NOT NULL,
    expires_at_micros  BIGINT       NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_acme_challenges_domain
    ON acme_challenges(domain_id);

CREATE TABLE IF NOT EXISTS ai_toolsets (
    id                VARCHAR(64)  PRIMARY KEY,
    org_id            VARCHAR(64)  NOT NULL,
    name              VARCHAR(255) NOT NULL,
    schema            JSONB        NOT NULL DEFAULT '{}'::JSONB,
    enabled           BOOLEAN      NOT NULL DEFAULT TRUE,
    created_at_micros BIGINT       NOT NULL,
    updated_at_micros BIGINT       NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_ai_toolsets_org_name
    ON ai_toolsets(org_id, name);
CREATE INDEX IF NOT EXISTS idx_ai_toolsets_org_updated
    ON ai_toolsets(org_id, updated_at_micros DESC);

-- ============================================================
-- pg_trgm GIN indexes（⌘K 远端聚合搜索）
-- ============================================================

CREATE INDEX IF NOT EXISTS gin_streams_name
    ON logical_streams USING gin (name gin_trgm_ops);
CREATE INDEX IF NOT EXISTS gin_dashboards_title
    ON dashboards USING gin (title gin_trgm_ops);
CREATE INDEX IF NOT EXISTS gin_saved_views_name
    ON saved_views USING gin (name gin_trgm_ops);
CREATE INDEX IF NOT EXISTS gin_alert_rules_name
    ON alert_rules USING gin (name gin_trgm_ops);
CREATE INDEX IF NOT EXISTS gin_incidents_fingerprint
    ON incidents USING gin (fingerprint gin_trgm_ops);
CREATE INDEX IF NOT EXISTS gin_sg_client_service
    ON service_graph_edges USING gin (client_service gin_trgm_ops);
CREATE INDEX IF NOT EXISTS gin_sg_server_service
    ON service_graph_edges USING gin (server_service gin_trgm_ops);

-- ============================================================
-- Federation secrets / semantic groups / incident RCA / mute / subscriptions
-- ============================================================

CREATE TABLE IF NOT EXISTS cluster_secrets (
    org_id            VARCHAR(64)  NOT NULL,
    ref_id            VARCHAR(255) NOT NULL,
    ciphertext        BYTEA        NOT NULL,
    nonce             BYTEA        NOT NULL,
    created_at_micros BIGINT       NOT NULL,
    updated_at_micros BIGINT       NOT NULL,
    PRIMARY KEY (org_id, ref_id)
);

CREATE TABLE IF NOT EXISTS semantic_groups (
    id                  VARCHAR(64)  PRIMARY KEY,
    org_id              VARCHAR(64)  NOT NULL,
    name                VARCHAR(255) NOT NULL,
    enabled             BOOLEAN      NOT NULL DEFAULT TRUE,
    -- [{ label, op (eq|neq|re|nre), value }]，全部命中才适用。
    matchers            JSONB        NOT NULL DEFAULT '[]'::jsonb,
    group_by            JSONB        NOT NULL DEFAULT '[]'::jsonb,
    created_at_micros   BIGINT       NOT NULL,
    updated_at_micros   BIGINT       NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_semantic_groups_org_enabled
    ON semantic_groups(org_id, enabled);

CREATE TABLE IF NOT EXISTS incident_rca (
    incident_id        VARCHAR(64) PRIMARY KEY,
    org_id             VARCHAR(64) NOT NULL,
    summary            TEXT        NOT NULL,
    provider           VARCHAR(32),
    model              VARCHAR(128),
    prompt_builtin_key VARCHAR(64),
    prompt_hash        VARCHAR(64),
    prompt_tokens      INTEGER     NOT NULL DEFAULT 0,
    completion_tokens  INTEGER     NOT NULL DEFAULT 0,
    finish_reason      VARCHAR(32),
    created_at_micros  BIGINT      NOT NULL,
    updated_at_micros  BIGINT      NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_incident_rca_org
    ON incident_rca(org_id, created_at_micros DESC);

-- 告警屏蔽：matchers 全命中 + window active 时暂停派发（incident 仍记录）。
CREATE TABLE IF NOT EXISTS mute_rules (
    id                  VARCHAR(64)  PRIMARY KEY,
    org_id              VARCHAR(64)  NOT NULL,
    name                VARCHAR(255) NOT NULL,
    enabled             BOOLEAN      NOT NULL DEFAULT TRUE,
    matchers            JSONB        NOT NULL DEFAULT '[]'::jsonb,
    -- MuteWindow：{type:fixed,start,end} | {type:recurring,timezone,weekday_mask,hour_start,hour_end}
    -- 列名用 time_window 而非 window（后者是 PG 保留字）。
    time_window         JSONB        NOT NULL,
    comment             TEXT         NOT NULL DEFAULT '',
    created_by          VARCHAR(64),
    created_at_micros   BIGINT       NOT NULL,
    updated_at_micros   BIGINT       NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_mute_rules_org_enabled
    ON mute_rules(org_id, enabled);

-- 告警订阅：matchers + min_severity 命中时，在升级目标之外追加通知。
CREATE TABLE IF NOT EXISTS alert_subscriptions (
    id                  VARCHAR(64)  PRIMARY KEY,
    org_id              VARCHAR(64)  NOT NULL,
    name                VARCHAR(255) NOT NULL,
    enabled             BOOLEAN      NOT NULL DEFAULT TRUE,
    matchers            JSONB        NOT NULL DEFAULT '[]'::jsonb,
    min_severity        VARCHAR(16),
    channel_ids         JSONB        NOT NULL DEFAULT '[]'::jsonb,
    user_ids            JSONB        NOT NULL DEFAULT '[]'::jsonb,
    created_at_micros   BIGINT       NOT NULL,
    updated_at_micros   BIGINT       NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_alert_subscriptions_org_enabled
    ON alert_subscriptions(org_id, enabled);

-- ============================================================
-- Org config: email domains / billing / trials
-- ============================================================

CREATE TABLE IF NOT EXISTS org_email_domains (
    org_id              VARCHAR(64)  NOT NULL,
    domain              VARCHAR(255) NOT NULL,
    created_at_micros   BIGINT       NOT NULL,
    PRIMARY KEY (org_id, domain)
);
CREATE INDEX IF NOT EXISTS idx_org_email_domains_org
    ON org_email_domains(org_id);

CREATE TABLE IF NOT EXISTS billing_settings (
    id                          VARCHAR(64)  PRIMARY KEY,
    enabled                     BOOLEAN      NOT NULL DEFAULT FALSE,
    signature_tolerance_secs    INTEGER      NOT NULL DEFAULT 300,
    webhook_secret_ciphertext   BYTEA,
    webhook_secret_nonce        BYTEA,
    api_key_ciphertext          BYTEA,
    api_key_nonce               BYTEA,
    updated_at_micros           BIGINT       NOT NULL
);

CREATE TABLE IF NOT EXISTS org_trials (
    org_id              VARCHAR(64)  PRIMARY KEY,
    started_at_micros   BIGINT       NOT NULL,
    ends_at_micros      BIGINT       NOT NULL,
    state               VARCHAR(16)  NOT NULL DEFAULT 'active',
    notified_stage      VARCHAR(16)  NOT NULL DEFAULT 'none',
    created_at_micros   BIGINT       NOT NULL,
    updated_at_micros   BIGINT       NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_org_trials_state ON org_trials(state);

-- ============================================================
-- Slow queries / search admission load
-- ============================================================

CREATE TABLE IF NOT EXISTS slow_queries (
    id                VARCHAR(64) PRIMARY KEY,
    org_id            VARCHAR(64) NOT NULL,
    fingerprint       VARCHAR(64) NOT NULL,
    language          VARCHAR(16) NOT NULL,
    statement         TEXT        NOT NULL,
    scanned_rows      BIGINT      NOT NULL DEFAULT 0,
    returned_rows     BIGINT      NOT NULL DEFAULT 0,
    took_ms           BIGINT      NOT NULL DEFAULT 0,
    time_range_secs   BIGINT,
    hit_count         BIGINT      NOT NULL DEFAULT 1,
    first_seen_micros BIGINT      NOT NULL,
    last_seen_micros  BIGINT      NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS uniq_slow_queries_org_fp
    ON slow_queries(org_id, fingerprint);
CREATE INDEX IF NOT EXISTS idx_slow_queries_org_last
    ON slow_queries(org_id, last_seen_micros DESC);

CREATE TABLE IF NOT EXISTS search_admission_load (
    node_id           VARCHAR(64) NOT NULL,
    work_group        VARCHAR(64) NOT NULL,
    in_flight         BIGINT      NOT NULL DEFAULT 0,
    updated_at_micros BIGINT      NOT NULL,
    PRIMARY KEY (node_id, work_group)
);
CREATE INDEX IF NOT EXISTS idx_search_admission_load_group
    ON search_admission_load(work_group, updated_at_micros DESC);

-- ============================================================
-- AI model providers / prompt templates / chat archives
-- ============================================================

CREATE TABLE IF NOT EXISTS ai_model_providers (
    id                 VARCHAR(64)  PRIMARY KEY,
    org_id             VARCHAR(64)  NOT NULL,
    provider           VARCHAR(32)  NOT NULL,
    name               VARCHAR(255) NOT NULL,
    base_url           TEXT,
    default_model      VARCHAR(128) NOT NULL,
    enabled            BOOLEAN      NOT NULL DEFAULT TRUE,
    timeout_ms         BIGINT       NOT NULL DEFAULT 30000,
    max_tokens         BIGINT,
    key_last4          VARCHAR(8),
    key_set            BOOLEAN      NOT NULL DEFAULT FALSE,
    created_at_micros  BIGINT       NOT NULL,
    updated_at_micros  BIGINT       NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_ai_providers_org
    ON ai_model_providers(org_id, updated_at_micros DESC);

CREATE TABLE IF NOT EXISTS ai_model_provider_secrets (
    provider_id        VARCHAR(64)  PRIMARY KEY,
    org_id             VARCHAR(64)  NOT NULL,
    ciphertext         BYTEA        NOT NULL,
    nonce              BYTEA        NOT NULL,
    created_at_micros  BIGINT       NOT NULL,
    updated_at_micros  BIGINT       NOT NULL
);

CREATE TABLE IF NOT EXISTS ai_prompt_templates (
    id                 VARCHAR(64)  PRIMARY KEY,
    org_id             VARCHAR(64),
    user_id            VARCHAR(64),
    scope              VARCHAR(16)  NOT NULL,
    builtin_key        VARCHAR(64),
    purpose            VARCHAR(32)  NOT NULL,
    name               VARCHAR(255) NOT NULL,
    body               TEXT         NOT NULL,
    variables_schema   JSONB        NOT NULL DEFAULT '{}'::JSONB,
    is_default         BOOLEAN      NOT NULL DEFAULT FALSE,
    enabled            BOOLEAN      NOT NULL DEFAULT TRUE,
    version            INTEGER      NOT NULL DEFAULT 1,
    parent_id          VARCHAR(64),
    created_by         VARCHAR(64),
    updated_by         VARCHAR(64),
    created_at_micros  BIGINT       NOT NULL,
    updated_at_micros  BIGINT       NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_ai_prompts_org_purpose
    ON ai_prompt_templates(org_id, purpose, enabled);
CREATE INDEX IF NOT EXISTS idx_ai_prompts_builtin_key
    ON ai_prompt_templates(builtin_key);
CREATE UNIQUE INDEX IF NOT EXISTS uniq_ai_prompts_builtin
    ON ai_prompt_templates(builtin_key) WHERE scope = 'builtin';

CREATE TABLE IF NOT EXISTS ai_chat_archives (
    id                 VARCHAR(64)  PRIMARY KEY,
    session_id         VARCHAR(64)  NOT NULL,
    org_id             VARCHAR(64)  NOT NULL,
    object_key         TEXT,
    sha256             VARCHAR(64),
    bytes              BIGINT       NOT NULL DEFAULT 0,
    status             VARCHAR(16)  NOT NULL,
    error              TEXT,
    created_by         VARCHAR(64),
    created_at_micros  BIGINT       NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_ai_archives_session
    ON ai_chat_archives(session_id, created_at_micros DESC);
CREATE INDEX IF NOT EXISTS idx_ai_archives_org
    ON ai_chat_archives(org_id, created_at_micros DESC);

-- ============================================================
-- Cross-cluster event bus（outbox / delivery cursor / org link / version / dedup）
-- ============================================================

CREATE TABLE IF NOT EXISTS cluster_event_outbox (
    seq               BIGSERIAL    PRIMARY KEY,
    id                VARCHAR(64)  NOT NULL UNIQUE,
    org_id            VARCHAR(64)  NOT NULL,
    event_type        VARCHAR(128) NOT NULL,
    subject           TEXT         NOT NULL,
    payload           JSONB        NOT NULL,
    created_at_micros BIGINT       NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_cluster_event_outbox_org ON cluster_event_outbox(org_id, seq);

CREATE TABLE IF NOT EXISTS cluster_event_delivery (
    remote_cluster_id VARCHAR(64) PRIMARY KEY,
    acked_seq         BIGINT      NOT NULL DEFAULT 0,
    updated_at_micros BIGINT      NOT NULL
);

CREATE TABLE IF NOT EXISTS cluster_org_link (
    remote_cluster_id VARCHAR(64) NOT NULL,
    local_org_id      VARCHAR(64) NOT NULL,
    remote_org_id     VARCHAR(64) NOT NULL,
    token_secret_ref  TEXT,
    PRIMARY KEY (remote_cluster_id, local_org_id)
);
CREATE INDEX IF NOT EXISTS idx_cluster_org_link_remote
    ON cluster_org_link(remote_cluster_id, remote_org_id);

CREATE TABLE IF NOT EXISTS cluster_resource_version (
    resource_kind     VARCHAR(64) NOT NULL,
    org_id            VARCHAR(64) NOT NULL,
    resource_id       VARCHAR(64) NOT NULL,
    version           BIGINT      NOT NULL,
    writer            VARCHAR(64) NOT NULL,
    updated_at_micros BIGINT      NOT NULL,
    PRIMARY KEY (resource_kind, org_id, resource_id)
);

CREATE TABLE IF NOT EXISTS seen_events (
    id                 VARCHAR(64) PRIMARY KEY,
    received_at_micros BIGINT      NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_seen_events_received ON seen_events(received_at_micros);

-- 已有表的补充索引（增量迁移引入）。
CREATE INDEX IF NOT EXISTS idx_domains_state ON domains(state);
CREATE INDEX IF NOT EXISTS idx_marketplace_subscriptions_org
    ON marketplace_subscriptions(org_id);

-- ============================================================
-- Seed data
-- ============================================================

-- model_prices seed（使用 src/model_pricing::default_seed 的值）
INSERT INTO model_prices (provider, model, prompt_usd_per_1k, completion_usd_per_1k, updated_at_micros)
VALUES
    ('openai',    'gpt-4o',             0.005,   0.015,   0),
    ('openai',    'gpt-4o-mini',        0.00015, 0.0006,  0),
    ('anthropic', 'claude-3-5-sonnet',  0.003,   0.015,   0),
    ('anthropic', 'claude-3-haiku',     0.00025, 0.00125, 0)
ON CONFLICT (provider, model) DO NOTHING;

-- AI builtin prompt templates（稳定 builtin_key；scope = builtin，不可变）。
INSERT INTO ai_prompt_templates
    (id, org_id, user_id, scope, builtin_key, purpose, name, body, variables_schema,
     is_default, enabled, version, parent_id, created_by, updated_by,
     created_at_micros, updated_at_micros)
VALUES
    ('78d508e3-303f-47c7-8d6f-9879ac2f10eb', NULL, NULL, 'builtin', 'system.default', 'system',
     'System Default',
     'You are MoleSignal AI, an observability investigation copilot. You help engineers analyze logs, metrics, traces, and alerts within the organization "{{org_name}}". You may only access tenant data through the provided backend tools; never assume or fabricate data. Always cite the evidence (stream, time range, row counts) behind each claim. When you are uncertain, say so. The current time is {{current_time}}.',
     '{"type":"object","properties":{"org_name":{"type":"string"},"current_time":{"type":"string"}}}'::JSONB,
     TRUE, TRUE, 1, NULL, 'system', 'system', 0, 0),

    ('65acbf0d-db56-4d32-ac5e-f42b0bceefa4', NULL, NULL, 'builtin', 'analysis.anomaly', 'anomaly_analysis',
     'Anomaly Analysis',
     'Analyze the selected time range {{time_range}} for anomalies across the relevant streams {{streams}}. Use the backend tools to inspect logs, metrics, and traces. Identify what changed: error-rate spikes, latency regressions, unusual log patterns, or volume shifts. Return a structured answer with: summary, anomaly_points (each with metric/stream, observed vs expected, timestamp), evidence, likely_causes, suggested_next_steps, related_links, and an overall confidence between 0 and 1.',
     '{"type":"object","properties":{"time_range":{"type":"string"},"streams":{"type":"string"}}}'::JSONB,
     TRUE, TRUE, 1, NULL, 'system', 'system', 0, 0),

    ('91437cdd-e596-4ac8-9f1a-9a0e8ebaba32', NULL, NULL, 'builtin', 'analysis.root_cause', 'root_cause',
     'Root Cause Analysis',
     'Perform a root-cause analysis for the incident in the time range {{time_range}} over streams {{streams}}. Correlate metrics, logs, and traces to build a causal chain from symptom to probable root cause. Prefer evidence-backed reasoning over speculation. Return a structured answer with: summary, anomaly_points, evidence (with tool, stream, and row counts), likely_causes ordered by likelihood, suggested_next_steps, related_links, and confidence between 0 and 1.',
     '{"type":"object","properties":{"time_range":{"type":"string"},"streams":{"type":"string"}}}'::JSONB,
     TRUE, TRUE, 1, NULL, 'system', 'system', 0, 0),

    ('7a0b7230-7ecd-454c-8347-cf8472323376', NULL, NULL, 'builtin', 'alert.explain', 'alert_explain',
     'Alert Explanation',
     'Explain the alert "{{alert_name}}" that fired during {{time_range}}. Describe in plain language what the alert measures, why it likely fired, what the blast radius is, and what an on-call engineer should check first. Use backend tools to confirm the current state. Return a structured answer with summary, evidence, likely_causes, suggested_next_steps, related_links, and confidence.',
     '{"type":"object","properties":{"alert_name":{"type":"string"},"time_range":{"type":"string"}}}'::JSONB,
     TRUE, TRUE, 1, NULL, 'system', 'system', 0, 0),

    ('a3494416-8926-46e5-8354-6f68288ac95b', NULL, NULL, 'builtin', 'query.generate', 'query_generation',
     'Query Generation',
     'Translate the user request into a correct query for MoleSignal over streams {{streams}} within {{time_range}}. Use SQL for logs/traces and PromQL for metrics. Prefer the backend query tools to validate the result. Return the final query plus a one-line explanation of what it returns. Never invent column or stream names; list streams first if unsure.',
     '{"type":"object","properties":{"streams":{"type":"string"},"time_range":{"type":"string"}}}'::JSONB,
     TRUE, TRUE, 1, NULL, 'system', 'system', 0, 0),

    ('9c68c7a1-5f0f-4a3b-9be8-7b1852241701', NULL, NULL, 'builtin', 'dashboard.authoring.v1', 'dashboard_authoring',
     'Dashboard Authoring v1',
     $dashboard_authoring$You are using the immutable `dashboard.authoring.v1` capability.

Create Dashboards only through the registered Dashboard authoring tools and the semantic authoring v1 contract. Never fabricate server-owned IDs, grid coordinates, query ref IDs, schema versions, organization/user identity, approval state, or a compiled Dashboard model.

Workflow:
1. If topic/data source/time range is missing, ask the user for it before calling preparation.
2. Discover current capabilities when a supported visualization or query combination is uncertain.
3. Call `prepare_dashboard` with one complete semantic specification. On structured validation errors, make at most one focused repair and prepare again.
4. Tell the user to review the persisted preview route. Never inline or ask the user to trust a model-generated compiled model.
5. Only after a valid preview, call `propose_dashboard_creation` with the draft ID, exact hash, reason, and impact. This creates a confirmation/approval request and never creates the Dashboard directly.
6. If proposal is unavailable, stop after preview and clearly state that creation must be completed outside this chat.

Do not use this capability to edit an existing Dashboard, explain an existing Dashboard, or answer an ordinary observability query that does not ask to create a Dashboard.$dashboard_authoring$,
     '{"type":"object","properties":{}}'::JSONB,
     TRUE, TRUE, 1, NULL, 'system', 'system', 0, 0)
ON CONFLICT (id) DO NOTHING;

-- Built-in VRL transform presets，统一进 functions 表（sentinel org `__builtin__`，只读，created_at=0 标记）。
INSERT INTO functions (id, org_id, name, language, source, params_schema, created_at_micros, updated_at_micros) VALUES
('2f0c86ee-6db4-496a-94aa-6f8c24b55920', '__builtin__', 'normalize-logs', 'vrl', $vrl$. = parse_json!(.message)

.environment = "production"
.cluster = "us-east-1"
.level = downcase(.level || "info")

if exists(.trace_id) {
    .trace.id = .trace_id
    del(.trace_id)
}$vrl$, '{"description":"解析 JSON message，补充环境/集群字段，统一 level 小写。"}'::jsonb, 0, 0),
('909f9e66-5c73-4ab1-98e6-9f0a68b06ad8', '__builtin__', 'route-by-service', 'vrl', $vrl$.service = downcase(to_string(.service) ?? "unknown")
.target_stream = "logs_" + .service$vrl$, '{"description":"归一化 service 名并派生每服务目标流名（logs_<service>）作为路由提示。"}'::jsonb, 0, 0),
('99f3af3c-4253-4798-824c-f7bb07a56ee9', '__builtin__', 'parse-key-value', 'vrl', $vrl$parsed = parse_key_value(to_string(.message) ?? "") ?? {}
. = merge(., parsed)$vrl$, '{"description":"把 logfmt / key=value 形式的 message 解析为字段后合并回事件。"}'::jsonb, 0, 0),
('30cb3f24-90b5-4def-a2f6-ce4676bc3ff5', '__builtin__', 'redact-email', 'vrl', $vrl$.message = replace(
    to_string(.message) ?? "",
    r'[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}',
    "[redacted-email]"
)$vrl$, '{"description":"把 message 中的邮箱地址替换为占位符（脱敏）。"}'::jsonb, 0, 0),
('a9b0a9f6-274b-48b6-9d6d-f122003e69c4', '__builtin__', 'add-intake-time', 'vrl', $vrl$.intake_at = to_unix_timestamp(now())$vrl$, '{"description":"附加摄取时间戳字段，便于排查端到端延迟。"}'::jsonb, 0, 0)
ON CONFLICT (id) DO NOTHING;

-- ============================================================
-- Password reset tokens
-- ============================================================

-- 一次性密码重置令牌。只存 SHA-256 摘要，不保存邮件中的原始令牌。
CREATE TABLE IF NOT EXISTS password_reset_tokens (
    id                  VARCHAR(64) PRIMARY KEY,
    user_id             VARCHAR(64) NOT NULL,
    token_hash          CHAR(64)    NOT NULL UNIQUE,
    created_at_micros   BIGINT      NOT NULL,
    expires_at_micros   BIGINT      NOT NULL,
    used_at_micros      BIGINT
);

CREATE INDEX IF NOT EXISTS idx_password_reset_tokens_user_created
    ON password_reset_tokens(user_id, created_at_micros DESC);
CREATE INDEX IF NOT EXISTS idx_password_reset_tokens_expiry
    ON password_reset_tokens(expires_at_micros)
    WHERE used_at_micros IS NULL;
CREATE UNIQUE INDEX IF NOT EXISTS uq_password_reset_tokens_user_active
    ON password_reset_tokens(user_id)
    WHERE used_at_micros IS NULL;

-- ============================================================
-- Hourly intake usage
-- ============================================================

-- Per-org hourly raw intake volume for operational overview pages.
--
-- `license_usage_daily` remains the billing ledger. This table is deliberately
-- narrower and time-bucketed so the Home page can answer "how much data was
-- received in this window?" without scanning telemetry payloads.
CREATE TABLE IF NOT EXISTS intake_usage_hourly (
    org_id                 VARCHAR(64) NOT NULL,
    bucket_start_micros    BIGINT      NOT NULL,
    intake_bytes           BIGINT      NOT NULL DEFAULT 0,
    PRIMARY KEY (org_id, bucket_start_micros)
);

CREATE INDEX IF NOT EXISTS idx_intake_usage_hourly_org_bucket
    ON intake_usage_hourly(org_id, bucket_start_micros DESC);

-- ============================================================
-- Extend table definitions
-- ============================================================

CREATE TABLE IF NOT EXISTS extend_table_definitions (
    org_id               VARCHAR(64)  NOT NULL,
    table_name           VARCHAR(255) NOT NULL,
    description          TEXT         NOT NULL DEFAULT '',
    key_field            VARCHAR(255) NOT NULL DEFAULT 'key',
    value_fields_json    JSONB        NOT NULL DEFAULT '[]'::JSONB,
    created_at_micros    BIGINT       NOT NULL,
    updated_at_micros    BIGINT       NOT NULL,
    PRIMARY KEY (org_id, table_name)
);

CREATE INDEX IF NOT EXISTS idx_extend_table_definitions_org_updated
    ON extend_table_definitions(org_id, updated_at_micros DESC);

-- ============================================================
-- Report templates
-- ============================================================

CREATE TABLE IF NOT EXISTS report_templates (
    id                  VARCHAR(64)  PRIMARY KEY,
    org_id              VARCHAR(64)  NOT NULL,
    name                VARCHAR(255) NOT NULL,
    description         TEXT         NOT NULL DEFAULT '',
    target_type         VARCHAR(16)  NOT NULL,
    format              VARCHAR(16)  NOT NULL,
    time_range_preset   VARCHAR(64)  NOT NULL,
    cron                VARCHAR(64)  NOT NULL DEFAULT 'every:7d',
    created_at_micros   BIGINT       NOT NULL,
    updated_at_micros   BIGINT       NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_report_template_org_name
    ON report_templates(org_id, name);

CREATE INDEX IF NOT EXISTS idx_report_template_org_updated
    ON report_templates(org_id, updated_at_micros DESC);

-- ============================================================
-- Report template content defaults
-- ============================================================

UPDATE report_templates
SET time_range_preset = CASE time_range_preset
    WHEN 'previous-1-hour' THEN 'previous-24-hours'
    WHEN 'previous-30-days' THEN 'previous-calendar-month'
    ELSE time_range_preset
END;

ALTER TABLE report_templates
    DROP COLUMN IF EXISTS cron;

-- ============================================================
-- Notify channel test status
-- ============================================================

ALTER TABLE notify_channels
    ADD COLUMN IF NOT EXISTS last_test_at_micros BIGINT,
    ADD COLUMN IF NOT EXISTS last_test_status VARCHAR(16),
    ADD COLUMN IF NOT EXISTS last_test_error TEXT;

ALTER TABLE notify_channels
    DROP CONSTRAINT IF EXISTS notify_channels_last_test_status_check;

ALTER TABLE notify_channels
    ADD CONSTRAINT notify_channels_last_test_status_check
    CHECK (last_test_status IS NULL OR last_test_status IN ('sent', 'failed'));

-- ============================================================
-- On-call schedule center
-- ============================================================

ALTER TABLE schedules
    ADD COLUMN IF NOT EXISTS description TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS team_id VARCHAR(64),
    ADD COLUMN IF NOT EXISTS enabled BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN IF NOT EXISTS created_by VARCHAR(64),
    ADD COLUMN IF NOT EXISTS updated_by VARCHAR(64);

CREATE INDEX IF NOT EXISTS idx_schedules_org_team
    ON schedules(org_id, team_id);

CREATE INDEX IF NOT EXISTS idx_schedules_org_enabled
    ON schedules(org_id, enabled);

-- ============================================================
-- Built-in function descriptions
-- ============================================================

-- Keep the built-in VRL function descriptions language-neutral for every
-- workspace. Preserve any future params_schema keys while replacing only the
-- human-readable description seeded above.
UPDATE functions
SET params_schema = COALESCE(params_schema, '{}'::JSONB) ||
    jsonb_build_object(
        'description',
        CASE name
            WHEN 'normalize-logs'
                THEN 'Parses the JSON message, adds environment and cluster fields, and normalizes level to lowercase.'
            WHEN 'route-by-service'
                THEN 'Normalizes the service name and derives a per-service target stream (logs_<service>) as a routing hint.'
            WHEN 'parse-key-value'
                THEN 'Parses a logfmt / key=value message into fields and merges them back into the event.'
            WHEN 'redact-email'
                THEN 'Replaces email addresses in message with a placeholder for redaction.'
            WHEN 'add-intake-time'
                THEN 'Adds an intake timestamp to help diagnose end-to-end latency.'
        END
    )
WHERE org_id = '__builtin__'
  AND name IN (
      'normalize-logs',
      'route-by-service',
      'parse-key-value',
      'redact-email',
      'add-intake-time'
  );

-- ============================================================
-- Mole Agent schema
-- ============================================================

-- Canonicalize Agent table names before creating the remaining schema.

ALTER TABLE chat_sessions RENAME TO agent_chats;
ALTER INDEX IF EXISTS idx_chat_sessions_org_user
    RENAME TO idx_agent_chats_org_user;
ALTER INDEX IF EXISTS idx_chat_sessions_org_user_active
    RENAME TO idx_agent_chats_org_user_active;

ALTER TABLE chat_messages RENAME TO agent_messages;
ALTER TABLE agent_messages RENAME COLUMN session_id TO chat_id;
ALTER INDEX IF EXISTS idx_chat_messages_session
    RENAME TO idx_agent_messages_chat;

ALTER TABLE ai_toolsets RENAME TO agent_toolsets;
ALTER INDEX IF EXISTS uq_ai_toolsets_org_name
    RENAME TO uq_agent_toolsets_org_name;
ALTER INDEX IF EXISTS idx_ai_toolsets_org_updated
    RENAME TO idx_agent_toolsets_org_updated;

ALTER TABLE ai_model_providers RENAME TO agent_model_providers;
ALTER INDEX IF EXISTS idx_ai_providers_org
    RENAME TO idx_agent_providers_org;

ALTER TABLE ai_model_provider_secrets
    RENAME TO agent_model_provider_secrets;

ALTER TABLE ai_prompt_templates RENAME TO agent_prompt_templates;
ALTER INDEX IF EXISTS idx_ai_prompts_org_purpose
    RENAME TO idx_agent_prompts_org_purpose;
ALTER INDEX IF EXISTS idx_ai_prompts_builtin_key
    RENAME TO idx_agent_prompts_builtin_key;
ALTER INDEX IF EXISTS uniq_ai_prompts_builtin
    RENAME TO uniq_agent_prompts_builtin;

ALTER TABLE ai_chat_archives RENAME TO agent_chat_archives;
ALTER TABLE agent_chat_archives RENAME COLUMN session_id TO chat_id;
ALTER INDEX IF EXISTS idx_ai_archives_session
    RENAME TO idx_agent_chat_archives_chat;
ALTER INDEX IF EXISTS idx_ai_archives_org
    RENAME TO idx_agent_chat_archives_org;

UPDATE agent_prompt_templates
SET body = 'You are Mole Agent, MoleSignal''s operations agent. You help engineers query observability data, analyze alerts, locate root causes, generate queries, inspect current on-call ownership, and propose remediation within the organization "{{org_name}}". Use only registered backend tools and authorized tenant data. Never fabricate evidence, access the public internet, invoke arbitrary HTTP, shell, browser, or open MCP tools, or execute a write without an approved operation. Cite the evidence and time range behind each claim. Distinguish verified facts, inferences, and suggestions; when evidence is insufficient, say so. The current time is {{current_time}}.',
    updated_by = 'system',
    updated_at_micros = GREATEST(updated_at_micros, 1)
WHERE scope = 'builtin'
  AND builtin_key = 'system.default';

CREATE TABLE IF NOT EXISTS agent_investigations (
    id                    VARCHAR(64) PRIMARY KEY,
    org_id                VARCHAR(64) NOT NULL,
    created_by            VARCHAR(64) NOT NULL,
    chat_id       VARCHAR(64),
    title                 VARCHAR(255) NOT NULL,
    status                VARCHAR(32) NOT NULL,
    context               JSONB NOT NULL DEFAULT '{}'::JSONB,
    summary               TEXT,
    confidence            VARCHAR(16),
    current_step          TEXT,
    started_at_micros     BIGINT,
    completed_at_micros   BIGINT,
    created_at_micros     BIGINT NOT NULL,
    updated_at_micros     BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_investigations_org_updated
    ON agent_investigations(org_id, updated_at_micros DESC);
CREATE INDEX IF NOT EXISTS idx_agent_investigations_status
    ON agent_investigations(org_id, status, updated_at_micros DESC);

CREATE TABLE IF NOT EXISTS agent_investigation_steps (
    id                    VARCHAR(64) PRIMARY KEY,
    investigation_id      VARCHAR(64) NOT NULL,
    org_id                VARCHAR(64) NOT NULL,
    position              INTEGER NOT NULL,
    title                 VARCHAR(255) NOT NULL,
    status                VARCHAR(32) NOT NULL,
    tool_name             VARCHAR(128),
    input                 JSONB NOT NULL DEFAULT '{}'::JSONB,
    output_summary        TEXT,
    conclusion_impact     TEXT,
    error                 TEXT,
    started_at_micros     BIGINT,
    ended_at_micros       BIGINT,
    created_at_micros     BIGINT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_agent_investigation_step_position
    ON agent_investigation_steps(investigation_id, position);

CREATE TABLE IF NOT EXISTS agent_investigation_evidence (
    id                    VARCHAR(64) PRIMARY KEY,
    investigation_id      VARCHAR(64) NOT NULL,
    step_id               VARCHAR(64),
    org_id                VARCHAR(64) NOT NULL,
    kind                  VARCHAR(32) NOT NULL,
    label                 VARCHAR(255) NOT NULL,
    fact_status           VARCHAR(32) NOT NULL,
    source_ref            JSONB NOT NULL DEFAULT '{}'::JSONB,
    query                 TEXT,
    parameters            JSONB NOT NULL DEFAULT '{}'::JSONB,
    summary               TEXT NOT NULL,
    created_at_micros     BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_evidence_investigation
    ON agent_investigation_evidence(investigation_id, created_at_micros);

CREATE TABLE IF NOT EXISTS agent_investigation_hypotheses (
    id                    VARCHAR(64) PRIMARY KEY,
    investigation_id      VARCHAR(64) NOT NULL,
    org_id                VARCHAR(64) NOT NULL,
    statement             TEXT NOT NULL,
    confidence            VARCHAR(16) NOT NULL,
    status                VARCHAR(32) NOT NULL,
    evidence_ids          JSONB NOT NULL DEFAULT '[]'::JSONB,
    created_at_micros     BIGINT NOT NULL,
    updated_at_micros     BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_hypotheses_investigation
    ON agent_investigation_hypotheses(investigation_id, updated_at_micros);

CREATE TABLE IF NOT EXISTS agent_automations (
    id                    VARCHAR(64) PRIMARY KEY,
    org_id                VARCHAR(64) NOT NULL,
    name                  VARCHAR(255) NOT NULL,
    description           TEXT NOT NULL DEFAULT '',
    enabled               BOOLEAN NOT NULL DEFAULT TRUE,
    trigger               JSONB NOT NULL,
    input_context         JSONB NOT NULL DEFAULT '{}'::JSONB,
    steps                 JSONB NOT NULL DEFAULT '[]'::JSONB,
    allowed_tools         JSONB NOT NULL DEFAULT '[]'::JSONB,
    approval_policy       JSONB NOT NULL DEFAULT '{}'::JSONB,
    output_actions        JSONB NOT NULL DEFAULT '[]'::JSONB,
    failure_policy        JSONB NOT NULL DEFAULT '{}'::JSONB,
    notification          JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_by            VARCHAR(64) NOT NULL,
    created_at_micros     BIGINT NOT NULL,
    updated_at_micros     BIGINT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_agent_automations_org_name
    ON agent_automations(org_id, name);

CREATE TABLE IF NOT EXISTS agent_approval_requests (
    id                    VARCHAR(64) PRIMARY KEY,
    org_id                VARCHAR(64) NOT NULL,
    investigation_id      VARCHAR(64),
    action                VARCHAR(128) NOT NULL,
    target                TEXT NOT NULL,
    parameters            JSONB NOT NULL,
    reason                TEXT NOT NULL,
    impact                TEXT NOT NULL,
    risk                  VARCHAR(8) NOT NULL,
    status                VARCHAR(16) NOT NULL,
    requested_by          VARCHAR(64) NOT NULL,
    required_approvals    INTEGER NOT NULL,
    reviews               JSONB NOT NULL DEFAULT '[]'::JSONB,
    expires_at_micros     BIGINT,
    decided_at_micros     BIGINT,
    created_at_micros     BIGINT NOT NULL,
    updated_at_micros     BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_approvals_org_status
    ON agent_approval_requests(org_id, status, created_at_micros DESC);

CREATE TABLE IF NOT EXISTS agent_executions (
    id                    VARCHAR(64) PRIMARY KEY,
    org_id                VARCHAR(64) NOT NULL,
    approval_request_id   VARCHAR(64) NOT NULL,
    investigation_id      VARCHAR(64),
    action                VARCHAR(128) NOT NULL,
    target                TEXT NOT NULL,
    parameters            JSONB NOT NULL,
    idempotency_key       VARCHAR(128) NOT NULL,
    requested_by          VARCHAR(64) NOT NULL,
    approved_by           JSONB NOT NULL DEFAULT '[]'::JSONB,
    status                VARCHAR(32) NOT NULL,
    output_summary        TEXT,
    error                 TEXT,
    verification          JSONB NOT NULL DEFAULT '{}'::JSONB,
    started_at_micros     BIGINT,
    finished_at_micros    BIGINT,
    created_at_micros     BIGINT NOT NULL,
    updated_at_micros     BIGINT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_agent_executions_idempotency
    ON agent_executions(org_id, idempotency_key);
CREATE UNIQUE INDEX IF NOT EXISTS uq_agent_executions_approval
    ON agent_executions(org_id, approval_request_id);
CREATE INDEX IF NOT EXISTS idx_agent_executions_org_created
    ON agent_executions(org_id, created_at_micros DESC);

CREATE TABLE IF NOT EXISTS agent_tool_calls (
    id                    VARCHAR(64) PRIMARY KEY,
    org_id                VARCHAR(64) NOT NULL,
    chat_id       VARCHAR(64),
    investigation_id      VARCHAR(64),
    step_id               VARCHAR(64),
    tool_name             VARCHAR(128) NOT NULL,
    risk                  VARCHAR(8) NOT NULL,
    input                 JSONB NOT NULL,
    output_summary        TEXT,
    status                VARCHAR(16) NOT NULL,
    error                 TEXT,
    duration_ms           BIGINT NOT NULL,
    called_by             VARCHAR(64) NOT NULL,
    created_at_micros     BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_tool_calls_org_created
    ON agent_tool_calls(org_id, created_at_micros DESC);
CREATE INDEX IF NOT EXISTS idx_agent_tool_calls_investigation
    ON agent_tool_calls(investigation_id, created_at_micros);

CREATE TABLE IF NOT EXISTS agent_contract_versions (
    contract_key          VARCHAR(128) NOT NULL,
    version               INTEGER NOT NULL,
    kind                  VARCHAR(32) NOT NULL,
    dialect               VARCHAR(128) NOT NULL,
    document              JSONB NOT NULL,
    schema_hash           VARCHAR(64) NOT NULL,
    status                VARCHAR(16) NOT NULL,
    published_at_micros   BIGINT NOT NULL,
    PRIMARY KEY (contract_key, version),
    CONSTRAINT agent_contract_versions_version_check
        CHECK (version > 0),
    CONSTRAINT agent_contract_versions_kind_check
        CHECK (kind IN ('dashboard_model', 'dashboard_authoring', 'visualization_manifest')),
    CONSTRAINT agent_contract_versions_status_check
        CHECK (status IN ('published', 'disabled')),
    CONSTRAINT agent_contract_versions_hash_check
        CHECK (char_length(schema_hash) = 64),
    CONSTRAINT uq_agent_contract_versions_hash
        UNIQUE (contract_key, version, schema_hash)
);

CREATE TABLE IF NOT EXISTS agent_capability_contract_bindings (
    capability_key             VARCHAR(128) PRIMARY KEY,
    revision                   BIGINT NOT NULL,
    model_contract_key         VARCHAR(128) NOT NULL,
    model_contract_version     INTEGER NOT NULL,
    model_schema_hash          VARCHAR(64) NOT NULL,
    authoring_contract_key     VARCHAR(128) NOT NULL,
    authoring_contract_version INTEGER NOT NULL,
    authoring_schema_hash      VARCHAR(64) NOT NULL,
    visualization_contract_key VARCHAR(128) NOT NULL,
    visualization_contract_version INTEGER NOT NULL,
    visualization_schema_hash VARCHAR(64) NOT NULL,
    compiler_version           VARCHAR(128) NOT NULL,
    enabled                    BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at_micros          BIGINT NOT NULL,
    CONSTRAINT agent_capability_bindings_revision_check
        CHECK (revision > 0),
    CONSTRAINT agent_capability_bindings_hash_check
        CHECK (
            char_length(model_schema_hash) = 64
            AND char_length(authoring_schema_hash) = 64
            AND char_length(visualization_schema_hash) = 64
        ),
    CONSTRAINT agent_capability_bindings_model_fk
        FOREIGN KEY (model_contract_key, model_contract_version, model_schema_hash)
        REFERENCES agent_contract_versions (contract_key, version, schema_hash),
    CONSTRAINT agent_capability_bindings_authoring_fk
        FOREIGN KEY (authoring_contract_key, authoring_contract_version, authoring_schema_hash)
        REFERENCES agent_contract_versions (contract_key, version, schema_hash),
    CONSTRAINT agent_capability_bindings_visualization_fk
        FOREIGN KEY (
            visualization_contract_key,
            visualization_contract_version,
            visualization_schema_hash
        )
        REFERENCES agent_contract_versions (contract_key, version, schema_hash)
);

CREATE TABLE IF NOT EXISTS agent_dashboard_drafts (
    id                       VARCHAR(64) PRIMARY KEY,
    org_id                   VARCHAR(64) NOT NULL,
    created_by               VARCHAR(64) NOT NULL,
    authoring_version        INTEGER NOT NULL,
    model_schema_version     INTEGER NOT NULL,
    compiler_version         VARCHAR(128) NOT NULL,
    contract_binding_revision BIGINT NOT NULL,
    authoring_schema_hash    VARCHAR(64) NOT NULL,
    model_schema_hash        VARCHAR(64) NOT NULL,
    visualization_schema_hash VARCHAR(64) NOT NULL,
    authoring_spec           JSONB NOT NULL,
    compiled_model           JSONB NOT NULL,
    model_hash               VARCHAR(64) NOT NULL,
    folder_id                VARCHAR(64),
    status                   VARCHAR(16) NOT NULL DEFAULT 'ready',
    dashboard_id             VARCHAR(64),
    warnings                 JSONB NOT NULL DEFAULT '[]'::JSONB,
    preflight                JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at_micros        BIGINT NOT NULL,
    expires_at_micros        BIGINT NOT NULL,
    consumed_at_micros       BIGINT,
    CONSTRAINT agent_dashboard_drafts_contract_revision_check
        CHECK (contract_binding_revision > 0),
    CONSTRAINT agent_dashboard_drafts_contract_hash_check
        CHECK (
            char_length(authoring_schema_hash) = 64
            AND char_length(model_schema_hash) = 64
            AND char_length(visualization_schema_hash) = 64
        ),
    CONSTRAINT agent_dashboard_drafts_status_check
        CHECK (status IN ('ready', 'consumed', 'expired')),
    CONSTRAINT agent_dashboard_drafts_consumption_check
        CHECK (
            (status = 'consumed' AND dashboard_id IS NOT NULL AND consumed_at_micros IS NOT NULL)
            OR (status <> 'consumed' AND dashboard_id IS NULL AND consumed_at_micros IS NULL)
        )
);
CREATE INDEX IF NOT EXISTS idx_agent_dashboard_drafts_org_status_expiry
    ON agent_dashboard_drafts(org_id, status, expires_at_micros);
CREATE INDEX IF NOT EXISTS idx_agent_dashboard_drafts_creator_created
    ON agent_dashboard_drafts(org_id, created_by, created_at_micros DESC);
CREATE UNIQUE INDEX IF NOT EXISTS uq_agent_dashboard_drafts_dashboard
    ON agent_dashboard_drafts(dashboard_id)
    WHERE dashboard_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS agent_profiles (
    id                         VARCHAR(64) PRIMARY KEY,
    org_id                     VARCHAR(64) NOT NULL,
    name                       VARCHAR(255) NOT NULL,
    description                TEXT NOT NULL DEFAULT '',
    model_provider_id          VARCHAR(64),
    model                      VARCHAR(128),
    allowed_tools              JSONB NOT NULL DEFAULT '[]'::JSONB,
    data_scope                 JSONB NOT NULL DEFAULT '{}'::JSONB,
    risk_policy               JSONB NOT NULL DEFAULT '{}'::JSONB,
    network_access             VARCHAR(16) NOT NULL DEFAULT 'blocked',
    max_context_tokens         INTEGER NOT NULL DEFAULT 32000,
    max_investigation_secs     INTEGER NOT NULL DEFAULT 1800,
    max_tool_calls             INTEGER NOT NULL DEFAULT 32,
    is_default                 BOOLEAN NOT NULL DEFAULT FALSE,
    enabled                    BOOLEAN NOT NULL DEFAULT TRUE,
    created_by                 VARCHAR(64) NOT NULL,
    created_at_micros          BIGINT NOT NULL,
    updated_at_micros          BIGINT NOT NULL,
    CONSTRAINT agent_profile_network_blocked
        CHECK (network_access = 'blocked')
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_agent_profiles_org_name
    ON agent_profiles(org_id, name);
CREATE UNIQUE INDEX IF NOT EXISTS uq_agent_profiles_org_default
    ON agent_profiles(org_id)
    WHERE is_default = TRUE;

-- ============================================================
-- Mole Agent network policy
-- ============================================================

ALTER TABLE agent_profiles
    DROP CONSTRAINT IF EXISTS agent_profile_network_blocked;

ALTER TABLE agent_profiles
    ADD CONSTRAINT agent_profile_network_access_check
    CHECK (network_access IN ('blocked', 'allowed'));

-- ============================================================
-- Mole Agent tool control
-- ============================================================

-- Mole Agent tool policy, MCP management, and call-audit control plane.

CREATE TABLE IF NOT EXISTS agent_tool_policies (
    org_id                 VARCHAR(64) NOT NULL,
    tool_name              VARCHAR(192) NOT NULL,
    enabled                BOOLEAN NOT NULL DEFAULT TRUE,
    execution_mode         VARCHAR(32) NOT NULL,
    environment_overrides  JSONB NOT NULL DEFAULT '{}'::JSONB,
    timeout_ms             BIGINT NOT NULL DEFAULT 30000,
    max_calls_per_run      INTEGER NOT NULL DEFAULT 32,
    max_response_bytes     BIGINT NOT NULL DEFAULT 1048576,
    updated_by             VARCHAR(64) NOT NULL,
    created_at_micros      BIGINT NOT NULL,
    updated_at_micros      BIGINT NOT NULL,
    PRIMARY KEY (org_id, tool_name)
);
CREATE INDEX IF NOT EXISTS idx_agent_tool_policies_org_updated
    ON agent_tool_policies(org_id, updated_at_micros DESC);

CREATE TABLE IF NOT EXISTS agent_tool_policy_defaults (
    org_id                 VARCHAR(64) PRIMARY KEY,
    risk_modes             JSONB NOT NULL DEFAULT '{
        "l0":"automatic",
        "l1":"confirmation",
        "l2":"single_approval",
        "l3":"dual_approval",
        "l4":"disabled"
    }'::JSONB,
    environment_overrides  JSONB NOT NULL DEFAULT '{}'::JSONB,
    updated_by             VARCHAR(64) NOT NULL,
    created_at_micros      BIGINT NOT NULL,
    updated_at_micros      BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS agent_mcp_servers (
    id                     VARCHAR(64) PRIMARY KEY,
    org_id                 VARCHAR(64) NOT NULL,
    name                   VARCHAR(255) NOT NULL,
    transport              VARCHAR(32) NOT NULL,
    endpoint_url           TEXT,
    command_template       VARCHAR(255),
    auth_type              VARCHAR(32) NOT NULL DEFAULT 'none',
    auth_header            VARCHAR(128),
    credential_last4       VARCHAR(16),
    credential_set         BOOLEAN NOT NULL DEFAULT FALSE,
    private_only           BOOLEAN NOT NULL DEFAULT TRUE,
    allowed_domains        JSONB NOT NULL DEFAULT '[]'::JSONB,
    allowed_cidrs          JSONB NOT NULL DEFAULT '[]'::JSONB,
    follow_redirects       BOOLEAN NOT NULL DEFAULT FALSE,
    tls_verify             BOOLEAN NOT NULL DEFAULT TRUE,
    timeout_ms             BIGINT NOT NULL DEFAULT 10000,
    max_response_bytes     BIGINT NOT NULL DEFAULT 1048576,
    enabled                BOOLEAN NOT NULL DEFAULT TRUE,
    status                 VARCHAR(32) NOT NULL DEFAULT 'unavailable',
    last_error             TEXT,
    last_tested_at_micros  BIGINT,
    last_synced_at_micros  BIGINT,
    created_by             VARCHAR(64) NOT NULL,
    created_at_micros      BIGINT NOT NULL,
    updated_at_micros      BIGINT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_agent_mcp_servers_org_name
    ON agent_mcp_servers(org_id, name);
CREATE INDEX IF NOT EXISTS idx_agent_mcp_servers_org_updated
    ON agent_mcp_servers(org_id, updated_at_micros DESC);

CREATE TABLE IF NOT EXISTS agent_mcp_server_secrets (
    server_id              VARCHAR(64) PRIMARY KEY,
    org_id                 VARCHAR(64) NOT NULL,
    ciphertext             BYTEA NOT NULL,
    nonce                  BYTEA NOT NULL,
    created_at_micros      BIGINT NOT NULL,
    updated_at_micros      BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_mcp_server_secrets_org
    ON agent_mcp_server_secrets(org_id);

CREATE TABLE IF NOT EXISTS agent_mcp_tools (
    id                     VARCHAR(64) PRIMARY KEY,
    org_id                 VARCHAR(64) NOT NULL,
    server_id              VARCHAR(64) NOT NULL,
    remote_name            VARCHAR(192) NOT NULL,
    name                   VARCHAR(192) NOT NULL,
    display_name           VARCHAR(255) NOT NULL,
    description            TEXT NOT NULL DEFAULT '',
    input_schema           JSONB NOT NULL DEFAULT '{"type":"object","properties":{}}'::JSONB,
    schema_hash            VARCHAR(64) NOT NULL,
    schema_dialect         VARCHAR(128) NOT NULL,
    schema_synced_at_micros BIGINT NOT NULL,
    unavailable_diagnostic TEXT,
    output_schema          JSONB,
    minimum_risk           VARCHAR(8) NOT NULL,
    risk                   VARCHAR(8) NOT NULL,
    execution_mode         VARCHAR(32) NOT NULL,
    capabilities           JSONB NOT NULL DEFAULT '{}'::JSONB,
    tags                   JSONB NOT NULL DEFAULT '[]'::JSONB,
    enabled                BOOLEAN NOT NULL DEFAULT FALSE,
    status                 VARCHAR(32) NOT NULL DEFAULT 'disabled',
    version                VARCHAR(128),
    last_synced_at_micros  BIGINT NOT NULL,
    created_at_micros      BIGINT NOT NULL,
    updated_at_micros      BIGINT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_agent_mcp_tools_server_remote
    ON agent_mcp_tools(server_id, remote_name);
CREATE UNIQUE INDEX IF NOT EXISTS uq_agent_mcp_tools_org_name
    ON agent_mcp_tools(org_id, name);
CREATE INDEX IF NOT EXISTS idx_agent_mcp_tools_org_server
    ON agent_mcp_tools(org_id, server_id, updated_at_micros DESC);

ALTER TABLE agent_tool_calls
    ADD COLUMN IF NOT EXISTS call_source VARCHAR(32) NOT NULL DEFAULT 'chat',
    ADD COLUMN IF NOT EXISTS profile_id VARCHAR(64),
    ADD COLUMN IF NOT EXISTS approval_id VARCHAR(64),
    ADD COLUMN IF NOT EXISTS policy_decision JSONB NOT NULL DEFAULT '{}'::JSONB,
    ADD COLUMN IF NOT EXISTS audit_id VARCHAR(64);

-- ============================================================
-- Inbound MCP Server and OAuth 2.1 authorization server
-- ============================================================

CREATE TABLE IF NOT EXISTS inbound_mcp_settings (
    org_id                 VARCHAR(64) PRIMARY KEY REFERENCES organizations(id) ON DELETE CASCADE,
    enabled                BOOLEAN NOT NULL DEFAULT TRUE,
    allowed_origins        JSONB NOT NULL DEFAULT '[]'::JSONB,
    max_request_bytes      BIGINT NOT NULL DEFAULT 1048576,
    max_response_bytes     BIGINT NOT NULL DEFAULT 1048576,
    max_concurrent_calls   INTEGER NOT NULL DEFAULT 8,
    calls_per_minute       INTEGER NOT NULL DEFAULT 60,
    read_timeout_ms        BIGINT NOT NULL DEFAULT 30000,
    updated_by             VARCHAR(64) NOT NULL REFERENCES users(id),
    created_at_micros      BIGINT NOT NULL,
    updated_at_micros      BIGINT NOT NULL,
    CONSTRAINT inbound_mcp_settings_request_limit
        CHECK (max_request_bytes BETWEEN 1024 AND 8388608),
    CONSTRAINT inbound_mcp_settings_response_limit
        CHECK (max_response_bytes BETWEEN 1024 AND 8388608),
    CONSTRAINT inbound_mcp_settings_concurrency_limit
        CHECK (max_concurrent_calls BETWEEN 1 AND 128),
    CONSTRAINT inbound_mcp_settings_rate_limit
        CHECK (calls_per_minute BETWEEN 1 AND 10000),
    CONSTRAINT inbound_mcp_settings_timeout_limit
        CHECK (read_timeout_ms BETWEEN 100 AND 300000)
);

CREATE TABLE IF NOT EXISTS inbound_mcp_oauth_clients (
    client_id                  TEXT PRIMARY KEY,
    client_name                VARCHAR(255) NOT NULL,
    redirect_uris              JSONB NOT NULL,
    grant_types                JSONB NOT NULL,
    response_types             JSONB NOT NULL,
    token_endpoint_auth_method VARCHAR(64) NOT NULL,
    scope                      TEXT NOT NULL,
    client_secret_hash         TEXT,
    client_id_issued_at        BIGINT NOT NULL,
    client_secret_expires_at   BIGINT,
    client_uri                 TEXT,
    software_id                VARCHAR(255),
    software_version           VARCHAR(255),
    created_at_micros          BIGINT NOT NULL,
    updated_at_micros          BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS inbound_mcp_oauth_codes (
    code_hash              VARCHAR(64) PRIMARY KEY,
    client_id              TEXT NOT NULL,
    org_id                 VARCHAR(64) NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id                VARCHAR(64) NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    redirect_uri           TEXT NOT NULL,
    scope                  TEXT NOT NULL,
    resource               TEXT NOT NULL,
    code_challenge         VARCHAR(128) NOT NULL,
    code_challenge_method  VARCHAR(16) NOT NULL,
    expires_at_micros      BIGINT NOT NULL,
    consumed_at_micros     BIGINT,
    created_at_micros      BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_inbound_mcp_oauth_codes_expiry
    ON inbound_mcp_oauth_codes(expires_at_micros)
    WHERE consumed_at_micros IS NULL;

CREATE TABLE IF NOT EXISTS inbound_mcp_oauth_tokens (
    id                     VARCHAR(64) PRIMARY KEY,
    token_hash             VARCHAR(64) NOT NULL UNIQUE,
    token_kind             VARCHAR(16) NOT NULL,
    family_id              VARCHAR(64) NOT NULL,
    client_id              TEXT NOT NULL,
    org_id                 VARCHAR(64) NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id                VARCHAR(64) NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    scope                  TEXT NOT NULL,
    resource               TEXT NOT NULL,
    expires_at_micros      BIGINT NOT NULL,
    revoked_at_micros      BIGINT,
    rotated_from           VARCHAR(64),
    created_at_micros      BIGINT NOT NULL,
    CONSTRAINT inbound_mcp_oauth_token_kind
        CHECK (token_kind IN ('access', 'refresh'))
);
CREATE INDEX IF NOT EXISTS idx_inbound_mcp_oauth_tokens_org_created
    ON inbound_mcp_oauth_tokens(org_id, created_at_micros DESC);
CREATE INDEX IF NOT EXISTS idx_inbound_mcp_oauth_tokens_family
    ON inbound_mcp_oauth_tokens(family_id);
CREATE INDEX IF NOT EXISTS idx_inbound_mcp_oauth_tokens_client
    ON inbound_mcp_oauth_tokens(client_id, revoked_at_micros);

CREATE TABLE IF NOT EXISTS inbound_mcp_idempotency (
    org_id                 VARCHAR(64) NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    principal_type         VARCHAR(32) NOT NULL,
    principal_id           VARCHAR(64) NOT NULL,
    idempotency_key        VARCHAR(128) NOT NULL,
    tool_name              VARCHAR(192) NOT NULL,
    request_hash           VARCHAR(64) NOT NULL,
    status                 VARCHAR(16) NOT NULL DEFAULT 'pending',
    approval_id            VARCHAR(64),
    result                 JSONB,
    lease_expires_at_micros BIGINT NOT NULL,
    created_at_micros      BIGINT NOT NULL,
    updated_at_micros      BIGINT NOT NULL,
    PRIMARY KEY (org_id, principal_type, principal_id, idempotency_key),
    CONSTRAINT inbound_mcp_idempotency_status
        CHECK (status IN ('pending', 'completed', 'failed'))
);
CREATE INDEX IF NOT EXISTS idx_inbound_mcp_idempotency_expiry
    ON inbound_mcp_idempotency(lease_expires_at_micros)
    WHERE status = 'pending';

CREATE TABLE IF NOT EXISTS inbound_mcp_tasks (
    id                     VARCHAR(64) PRIMARY KEY,
    org_id                 VARCHAR(64) NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    principal_type         VARCHAR(32) NOT NULL,
    principal_id           VARCHAR(64) NOT NULL,
    tool_name              VARCHAR(192) NOT NULL,
    request                JSONB NOT NULL,
    status                 VARCHAR(32) NOT NULL,
    status_message         TEXT,
    input_requests         JSONB,
    input_responses        JSONB,
    result                 JSONB,
    error                  JSONB,
    cancel_requested       BOOLEAN NOT NULL DEFAULT FALSE,
    expires_at_micros      BIGINT NOT NULL,
    created_at_micros      BIGINT NOT NULL,
    updated_at_micros      BIGINT NOT NULL,
    CONSTRAINT inbound_mcp_task_status
        CHECK (status IN ('working', 'input_required', 'completed', 'failed', 'cancelled'))
);
CREATE INDEX IF NOT EXISTS idx_inbound_mcp_tasks_principal
    ON inbound_mcp_tasks(org_id, principal_type, principal_id, updated_at_micros DESC);
CREATE INDEX IF NOT EXISTS idx_inbound_mcp_tasks_expiry
    ON inbound_mcp_tasks(expires_at_micros);

-- ============================================================
-- User preference product fields
-- ============================================================

ALTER TABLE user_preferences
    ADD COLUMN IF NOT EXISTS date_format VARCHAR(32) NOT NULL DEFAULT 'yyyy_mm_dd_dash';

ALTER TABLE user_preferences
    ALTER COLUMN theme SET DEFAULT 'system';

ALTER TABLE user_preferences
    DROP CONSTRAINT IF EXISTS chk_user_preferences_theme;

ALTER TABLE user_preferences
    ADD CONSTRAINT chk_user_preferences_theme
        CHECK (theme IN ('system', 'dark', 'light'));

ALTER TABLE user_preferences
    ADD CONSTRAINT chk_user_preferences_date_format
        CHECK (
            date_format IN (
                'yyyy_mm_dd_dash',
                'yyyy_mm_dd_slash',
                'dd_mm_yyyy_slash',
                'mm_dd_yyyy_slash'
            )
        );

-- ============================================================
-- User profile and startup preferences
-- ============================================================

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS bio TEXT NOT NULL DEFAULT '';

ALTER TABLE user_preferences
    DROP CONSTRAINT IF EXISTS chk_user_preferences_default_home_route;

ALTER TABLE user_preferences
    ADD CONSTRAINT chk_user_preferences_default_home_route
        CHECK (
            default_home_route = 'last_visited'
            OR (
                default_home_route LIKE '/%'
                AND default_home_route NOT LIKE '//%'
            )
        );

-- ============================================================
-- Workspace preference defaults
-- ============================================================

CREATE TABLE IF NOT EXISTS workspace_preference_defaults (
    org_id                      VARCHAR(64) PRIMARY KEY,
    theme                       VARCHAR(16) NOT NULL DEFAULT 'system',
    density                     VARCHAR(16) NOT NULL DEFAULT 'normal',
    language                    VARCHAR(16) NOT NULL DEFAULT 'en-us',
    default_home_route          TEXT        NOT NULL DEFAULT '/home',
    time_format                 VARCHAR(16) NOT NULL DEFAULT 'iso_24h',
    date_format                 VARCHAR(32) NOT NULL DEFAULT 'yyyy_mm_dd_dash',
    timezone                    VARCHAR(64) NOT NULL DEFAULT '',
    keyboard_shortcuts_enabled  BOOLEAN     NOT NULL DEFAULT TRUE,
    updated_at_micros           BIGINT      NOT NULL,
    CONSTRAINT chk_workspace_preference_defaults_theme
        CHECK (theme IN ('system', 'dark', 'light')),
    CONSTRAINT chk_workspace_preference_defaults_density
        CHECK (density IN ('compact', 'normal', 'comfortable')),
    CONSTRAINT chk_workspace_preference_defaults_language
        CHECK (language IN ('en-us', 'zh-cn')),
    CONSTRAINT chk_workspace_preference_defaults_time_format
        CHECK (time_format IN ('iso_24h', 'local_12h')),
    CONSTRAINT chk_workspace_preference_defaults_date_format
        CHECK (
            date_format IN (
                'yyyy_mm_dd_dash',
                'yyyy_mm_dd_slash',
                'dd_mm_yyyy_slash',
                'mm_dd_yyyy_slash'
            )
        ),
    CONSTRAINT chk_workspace_preference_defaults_home
        CHECK (
            default_home_route = 'last_visited'
            OR (
                default_home_route LIKE '/%'
                AND default_home_route NOT LIKE '//%'
            )
        )
);

-- ============================================================
-- Retired Actions cleanup
-- ============================================================

-- Remove the retired Actions capability.
--
-- Remove retired action targets before the canonical runtime reads the JSONB
-- policy with the reduced target enum.
UPDATE escalation_policies AS policy
SET steps = (
    SELECT COALESCE(
        jsonb_agg(
            jsonb_set(
                step_item.step,
                '{targets}',
                COALESCE(
                    (
                        SELECT jsonb_agg(target_item.target ORDER BY target_item.ordinality)
                        FROM jsonb_array_elements(
                            COALESCE(step_item.step->'targets', '[]'::jsonb)
                        ) WITH ORDINALITY AS target_item(target, ordinality)
                        WHERE target_item.target->>'kind' IS DISTINCT FROM 'action'
                    ),
                    '[]'::jsonb
                )
            )
            ORDER BY step_item.ordinality
        ),
        '[]'::jsonb
    )
    FROM jsonb_array_elements(policy.steps)
        WITH ORDINALITY AS step_item(step, ordinality)
)
WHERE EXISTS (
    SELECT 1
    FROM jsonb_array_elements(policy.steps) AS step_item(step)
    CROSS JOIN LATERAL jsonb_array_elements(
        COALESCE(step_item.step->'targets', '[]'::jsonb)
    ) AS target_item(target)
    WHERE target_item.target->>'kind' = 'action'
);

DROP TABLE IF EXISTS action_executions;
DROP TABLE IF EXISTS actions;

-- ============================================================
-- Distributed tracing system scope
-- ============================================================

-- System scope, immutable system resources, platform administrators, License history,
-- and dynamic Trace policy.

ALTER TABLE organizations
    ADD COLUMN IF NOT EXISTS system BOOLEAN NOT NULL DEFAULT FALSE;

CREATE UNIQUE INDEX IF NOT EXISTS uq_organizations_single_system
    ON organizations (system) WHERE system;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'chk_organizations_system_identity'
    ) THEN
        ALTER TABLE organizations
            ADD CONSTRAINT chk_organizations_system_identity
            CHECK (NOT system OR (name = '_sys' AND slug = '_sys' AND NOT disabled));
    END IF;
END
$$;

CREATE OR REPLACE FUNCTION protect_last_enabled_tenant_organization()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    remaining BIGINT;
BEGIN
    IF NOT OLD.system
        AND NOT OLD.disabled
        AND (TG_OP = 'DELETE' OR (TG_OP = 'UPDATE' AND NEW.disabled)) THEN
        PERFORM pg_advisory_xact_lock(hashtext('molesignal.organization.status'));
        SELECT COUNT(*) INTO remaining
          FROM organizations
         WHERE NOT system
           AND NOT disabled
           AND id <> OLD.id;
        IF remaining = 0 THEN
            RAISE EXCEPTION 'cannot remove or disable the last enabled tenant organization';
        END IF;
    END IF;
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END
$$;

DROP TRIGGER IF EXISTS trg_protect_last_enabled_tenant_organization ON organizations;
CREATE TRIGGER trg_protect_last_enabled_tenant_organization
BEFORE UPDATE OF disabled OR DELETE ON organizations
FOR EACH ROW EXECUTE FUNCTION protect_last_enabled_tenant_organization();

CREATE TABLE IF NOT EXISTS platform_administrators (
    user_id             VARCHAR(64) PRIMARY KEY,
    active              BOOLEAN NOT NULL DEFAULT TRUE,
    granted_by          VARCHAR(64),
    granted_at_micros   BIGINT NOT NULL,
    revoked_by          VARCHAR(64),
    revoked_at_micros   BIGINT
);
CREATE INDEX IF NOT EXISTS idx_platform_administrators_active
    ON platform_administrators (active) WHERE active;

CREATE TABLE IF NOT EXISTS license_versions (
    id                  VARCHAR(64) PRIMARY KEY,
    system_org_id       VARCHAR(64) NOT NULL,
    signed_package      JSONB NOT NULL,
    payload_digest      VARCHAR(64) NOT NULL,
    summary             JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_by          VARCHAR(64),
    created_at_micros   BIGINT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_license_versions_digest
    ON license_versions (payload_digest);

CREATE TABLE IF NOT EXISTS active_license_version (
    singleton_id        SMALLINT PRIMARY KEY DEFAULT 1,
    version_id          VARCHAR(64) NOT NULL,
    activated_by        VARCHAR(64),
    activated_at_micros BIGINT NOT NULL,
    CONSTRAINT chk_active_license_singleton CHECK (singleton_id = 1)
);

CREATE TABLE IF NOT EXISTS trace_runtime_policies (
    id                  VARCHAR(64) PRIMARY KEY,
    system_org_id       VARCHAR(64) NOT NULL,
    version             BIGINT NOT NULL UNIQUE,
    policy              JSONB NOT NULL,
    created_by          VARCHAR(64),
    created_at_micros   BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS active_trace_runtime_policy (
    singleton_id        SMALLINT PRIMARY KEY DEFAULT 1,
    policy_id           VARCHAR(64) NOT NULL,
    activated_by        VARCHAR(64),
    activated_at_micros BIGINT NOT NULL,
    CONSTRAINT chk_active_trace_policy_singleton CHECK (singleton_id = 1)
);

CREATE TABLE IF NOT EXISTS trace_debug_tokens (
    id                  VARCHAR(64) PRIMARY KEY,
    token_hash          VARCHAR(128) NOT NULL UNIQUE,
    organization_id     VARCHAR(64),
    route_pattern       VARCHAR(255),
    expires_at_micros   BIGINT NOT NULL,
    max_uses            BIGINT NOT NULL DEFAULT 1,
    used_count          BIGINT NOT NULL DEFAULT 0,
    revoked_at_micros   BIGINT,
    created_by          VARCHAR(64) NOT NULL,
    created_at_micros   BIGINT NOT NULL
);

CREATE OR REPLACE FUNCTION protect_system_organization()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'UPDATE' AND OLD.system THEN
        IF NEW.id IS DISTINCT FROM OLD.id
            OR NEW.name IS DISTINCT FROM OLD.name
            OR NEW.slug IS DISTINCT FROM OLD.slug
            OR NEW.system IS DISTINCT FROM OLD.system THEN
            RAISE EXCEPTION 'immutable system organization';
        END IF;
    ELSIF TG_OP = 'DELETE' AND OLD.system THEN
        RAISE EXCEPTION 'system organization cannot be deleted';
    END IF;
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END
$$;

DROP TRIGGER IF EXISTS trg_protect_system_organization ON organizations;
CREATE TRIGGER trg_protect_system_organization
BEFORE UPDATE OR DELETE ON organizations
FOR EACH ROW EXECUTE FUNCTION protect_system_organization();

CREATE OR REPLACE FUNCTION reject_system_organization_membership()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF EXISTS (SELECT 1 FROM organizations WHERE id = NEW.org_id AND system) THEN
        RAISE EXCEPTION 'system organization does not accept membership';
    END IF;
    RETURN NEW;
END
$$;

DROP TRIGGER IF EXISTS trg_reject_system_membership ON memberships;
CREATE TRIGGER trg_reject_system_membership
BEFORE INSERT OR UPDATE ON memberships
FOR EACH ROW EXECUTE FUNCTION reject_system_organization_membership();

DROP TRIGGER IF EXISTS trg_reject_system_team ON teams;
CREATE TRIGGER trg_reject_system_team
BEFORE INSERT OR UPDATE ON teams
FOR EACH ROW EXECUTE FUNCTION reject_system_organization_membership();

CREATE OR REPLACE FUNCTION protect_system_stream()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    org_is_system BOOLEAN;
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF OLD.system THEN
            RAISE EXCEPTION 'system stream cannot be deleted';
        END IF;
        RETURN OLD;
    END IF;

    SELECT system INTO org_is_system FROM organizations WHERE id = NEW.org_id;
    IF NEW.system OR NEW.name = '_molesignal' OR COALESCE(org_is_system, FALSE) THEN
        IF NOT NEW.system
            OR NEW.name <> '_molesignal'
            OR NOT COALESCE(org_is_system, FALSE) THEN
            RAISE EXCEPTION 'invalid system stream identity';
        END IF;
    END IF;

    IF TG_OP = 'UPDATE' AND OLD.system THEN
        IF NEW.id IS DISTINCT FROM OLD.id
            OR NEW.org_id IS DISTINCT FROM OLD.org_id
            OR NEW.name IS DISTINCT FROM OLD.name
            OR NEW.stream_type IS DISTINCT FROM OLD.stream_type
            OR NEW.system IS DISTINCT FROM OLD.system THEN
            RAISE EXCEPTION 'immutable system stream identity';
        END IF;
        IF NEW.schema IS DISTINCT FROM OLD.schema
            AND COALESCE(
                current_setting('molesignal.internal_system_mutation', TRUE),
                'false'
            ) <> 'true' THEN
            RAISE EXCEPTION 'system stream schema may only evolve through internal intake';
        END IF;
    END IF;
    RETURN NEW;
END
$$;

DROP TRIGGER IF EXISTS trg_protect_system_stream ON logical_streams;
CREATE TRIGGER trg_protect_system_stream
BEFORE INSERT OR UPDATE OR DELETE ON logical_streams
FOR EACH ROW EXECUTE FUNCTION protect_system_stream();

CREATE OR REPLACE FUNCTION reject_license_version_mutation()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'License versions are immutable';
END
$$;

DROP TRIGGER IF EXISTS trg_license_versions_immutable ON license_versions;
CREATE TRIGGER trg_license_versions_immutable
BEFORE UPDATE OR DELETE ON license_versions
FOR EACH ROW EXECUTE FUNCTION reject_license_version_mutation();

CREATE OR REPLACE FUNCTION protect_last_platform_administrator()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    remaining BIGINT;
BEGIN
    IF (TG_OP = 'DELETE' AND OLD.active)
        OR (TG_OP = 'UPDATE' AND OLD.active AND NOT NEW.active) THEN
        SELECT COUNT(*) INTO remaining
        FROM platform_administrators
        WHERE active AND user_id <> OLD.user_id;
        IF remaining = 0 THEN
            RAISE EXCEPTION 'cannot remove the last active platform administrator';
        END IF;
    END IF;
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END
$$;

DROP TRIGGER IF EXISTS trg_protect_last_platform_administrator ON platform_administrators;
CREATE TRIGGER trg_protect_last_platform_administrator
BEFORE UPDATE OR DELETE ON platform_administrators
FOR EACH ROW EXECUTE FUNCTION protect_last_platform_administrator();

CREATE OR REPLACE FUNCTION protect_platform_administrator_user()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    is_platform_admin BOOLEAN;
    remaining BIGINT;
BEGIN
    SELECT active INTO is_platform_admin
    FROM platform_administrators
    WHERE user_id = OLD.id;
    IF COALESCE(is_platform_admin, FALSE)
        AND (TG_OP = 'DELETE' OR (TG_OP = 'UPDATE' AND NEW.disabled)) THEN
        SELECT COUNT(*) INTO remaining
        FROM platform_administrators pa
        JOIN users u ON u.id = pa.user_id
        WHERE pa.active AND NOT u.disabled AND pa.user_id <> OLD.id;
        IF remaining = 0 THEN
            RAISE EXCEPTION 'cannot make the last platform administrator unusable';
        END IF;
    END IF;
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END
$$;

DROP TRIGGER IF EXISTS trg_protect_platform_administrator_user ON users;
CREATE TRIGGER trg_protect_platform_administrator_user
BEFORE UPDATE OR DELETE ON users
FOR EACH ROW EXECUTE FUNCTION protect_platform_administrator_user();

-- ============================================================
-- Search job Trace links
-- ============================================================

-- Persist only bounded W3C correlation for delayed search/backfill execution.
-- This value is diagnostic linkage and MUST NOT be used as an authorization source.
ALTER TABLE search_jobs
    ADD COLUMN IF NOT EXISTS trace_link JSONB,
    ADD COLUMN IF NOT EXISTS attempt INTEGER NOT NULL DEFAULT 0;

-- ============================================================
-- Unified IAM access
-- ============================================================

-- Unified IAM access: role bindings, relationships, explicit cross-org grants,
-- and version-based invalidation. Canonicalize the IAM table names before
-- creating the remaining tables and constraints.
DO $$
BEGIN
    IF to_regclass('iam_memberships') IS NULL
        AND to_regclass('memberships') IS NOT NULL THEN
        EXECUTE 'ALTER TABLE memberships RENAME TO iam_memberships';
    END IF;
    IF to_regclass('iam_platform_administrators') IS NULL
        AND to_regclass('platform_administrators') IS NOT NULL THEN
        EXECUTE 'ALTER TABLE platform_administrators RENAME TO iam_platform_administrators';
    END IF;
    IF to_regclass('idx_iam_memberships_org') IS NULL
        AND to_regclass('idx_memberships_org') IS NOT NULL THEN
        EXECUTE 'ALTER INDEX idx_memberships_org RENAME TO idx_iam_memberships_org';
    END IF;
    IF to_regclass('idx_iam_platform_administrators_active') IS NULL
        AND to_regclass('idx_platform_administrators_active') IS NOT NULL THEN
        EXECUTE
            'ALTER INDEX idx_platform_administrators_active '
            'RENAME TO idx_iam_platform_administrators_active';
    END IF;
END
$$;

-- Refresh the protection functions after canonicalizing the platform
-- administrator table name.
CREATE OR REPLACE FUNCTION protect_last_platform_administrator()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    remaining BIGINT;
BEGIN
    IF (TG_OP = 'DELETE' AND OLD.active)
        OR (TG_OP = 'UPDATE' AND OLD.active AND NOT NEW.active) THEN
        SELECT COUNT(*) INTO remaining
        FROM iam_platform_administrators
        WHERE active AND user_id <> OLD.user_id;
        IF remaining = 0 THEN
            RAISE EXCEPTION 'cannot remove the last active platform administrator';
        END IF;
    END IF;
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END
$$;

CREATE OR REPLACE FUNCTION protect_platform_administrator_user()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    is_platform_admin BOOLEAN;
    remaining BIGINT;
BEGIN
    SELECT active INTO is_platform_admin
    FROM iam_platform_administrators
    WHERE user_id = OLD.id;
    IF COALESCE(is_platform_admin, FALSE)
        AND (TG_OP = 'DELETE' OR (TG_OP = 'UPDATE' AND NEW.disabled)) THEN
        SELECT COUNT(*) INTO remaining
        FROM iam_platform_administrators pa
        JOIN users u ON u.id = pa.user_id
        WHERE pa.active AND NOT u.disabled AND pa.user_id <> OLD.id;
        IF remaining = 0 THEN
            RAISE EXCEPTION 'cannot make the last platform administrator unusable';
        END IF;
    END IF;
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END
$$;

ALTER TABLE iam_roles
    ADD COLUMN IF NOT EXISTS role_type VARCHAR(16) NOT NULL DEFAULT 'organization',
    ADD COLUMN IF NOT EXISTS scope VARCHAR(16) NOT NULL DEFAULT 'organization';

ALTER TABLE iam_role_permissions
    ALTER COLUMN permission_key TYPE VARCHAR(128);

-- Keep role keys until the built-in role rows are materialized, then resolve
-- them into the stable role_id columns.
ALTER TABLE api_tokens
    ADD COLUMN IF NOT EXISTS role_id VARCHAR(64);
ALTER TABLE invitations
    ADD COLUMN IF NOT EXISTS role_id VARCHAR(64);

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'chk_iam_roles_role_type'
    ) THEN
        ALTER TABLE iam_roles
            ADD CONSTRAINT chk_iam_roles_role_type
            CHECK (role_type IN ('platform', 'organization', 'resource'));
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'chk_iam_roles_scope'
    ) THEN
        ALTER TABLE iam_roles
            ADD CONSTRAINT chk_iam_roles_scope
            CHECK (scope IN ('platform', 'organization', 'resource'));
    END IF;
END
$$;

CREATE TABLE IF NOT EXISTS iam_builtin_roles (
    role_key            VARCHAR(64)  PRIMARY KEY,
    name                VARCHAR(128) NOT NULL,
    description         TEXT         NOT NULL,
    role_type           VARCHAR(16)  NOT NULL,
    scope               VARCHAR(16)  NOT NULL,
    display_priority    INTEGER      NOT NULL,
    CONSTRAINT chk_iam_builtin_roles_role_type
        CHECK (role_type IN ('platform', 'organization', 'resource')),
    CONSTRAINT chk_iam_builtin_roles_scope
        CHECK (scope IN ('platform', 'organization', 'resource'))
);

INSERT INTO iam_builtin_roles (
    role_key, name, description, role_type, scope, display_priority
)
VALUES
    ('platform_owner', 'Owner', 'Full platform administrative access.', 'platform', 'platform', 5),
    ('owner', 'Owner', 'Full administrative access.', 'organization', 'organization', 10),
    ('admin', 'Admin', 'Administrative access for day-to-day operations.', 'organization', 'organization', 20),
    ('editor', 'Editor', 'Can operate and change product resources.', 'organization', 'organization', 30),
    ('viewer', 'Viewer', 'Read-only product access.', 'organization', 'organization', 40),
    ('intake', 'Intake token', 'Write-only access for telemetry intake.', 'organization', 'organization', 80),
    ('rum_client', 'RUM client', 'Application-bound write-only access for RUM clients.', 'organization', 'organization', 90)
ON CONFLICT (role_key) DO UPDATE
SET name = EXCLUDED.name,
    description = EXCLUDED.description,
    role_type = EXCLUDED.role_type,
    scope = EXCLUDED.scope,
    display_priority = EXCLUDED.display_priority;

CREATE TABLE IF NOT EXISTS iam_builtin_role_purposes (
    purpose             VARCHAR(64) PRIMARY KEY,
    role_key            VARCHAR(64) NOT NULL
        REFERENCES iam_builtin_roles(role_key) ON DELETE RESTRICT
);

INSERT INTO iam_builtin_role_purposes (purpose, role_key)
VALUES
    ('platform_administrator', 'platform_owner'),
    ('organization_bootstrap', 'owner'),
    ('self_service_signup', 'viewer'),
    ('default_api_token', 'intake'),
    ('rum_client_token', 'rum_client')
ON CONFLICT (purpose) DO UPDATE
SET role_key = EXCLUDED.role_key;

CREATE TABLE IF NOT EXISTS iam_policy_versions (
    organization_id     VARCHAR(64) PRIMARY KEY,
    version             BIGINT      NOT NULL DEFAULT 1 CHECK (version > 0),
    updated_at_micros   BIGINT      NOT NULL
);

INSERT INTO iam_policy_versions (organization_id, version, updated_at_micros)
SELECT id, 1, (EXTRACT(EPOCH FROM clock_timestamp()) * 1000000)::BIGINT
FROM organizations
ON CONFLICT (organization_id) DO NOTHING;

CREATE TABLE IF NOT EXISTS iam_role_bindings (
    id                  VARCHAR(64) PRIMARY KEY,
    organization_id     VARCHAR(64) NOT NULL,
    role_id             VARCHAR(64) NOT NULL,
    principal_type      VARCHAR(24) NOT NULL,
    principal_id        VARCHAR(64) NOT NULL,
    resource_type       VARCHAR(64),
    resource_id         VARCHAR(255),
    conditions          JSONB       NOT NULL DEFAULT '{}'::JSONB,
    starts_at_micros    BIGINT,
    expires_at_micros   BIGINT,
    created_by          VARCHAR(64) NOT NULL,
    created_at_micros   BIGINT      NOT NULL,
    CONSTRAINT chk_iam_role_bindings_principal
        CHECK (principal_type IN ('user', 'team', 'group', 'service_account', 'organization')),
    CONSTRAINT chk_iam_role_bindings_resource
        CHECK (
            (resource_type IS NULL AND resource_id IS NULL)
            OR (resource_type IS NOT NULL AND resource_id IS NOT NULL)
        ),
    CONSTRAINT chk_iam_role_bindings_window
        CHECK (
            starts_at_micros IS NULL
            OR expires_at_micros IS NULL
            OR starts_at_micros < expires_at_micros
        )
);
CREATE INDEX IF NOT EXISTS idx_iam_role_bindings_principal
    ON iam_role_bindings (organization_id, principal_type, principal_id);
CREATE INDEX IF NOT EXISTS idx_iam_role_bindings_role
    ON iam_role_bindings (organization_id, role_id);
CREATE INDEX IF NOT EXISTS idx_iam_role_bindings_resource
    ON iam_role_bindings (organization_id, resource_type, resource_id)
    WHERE resource_type IS NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS uq_iam_roles_org_id
    ON iam_roles (org_id, id);

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_iam_role_bindings_role'
    ) THEN
        ALTER TABLE iam_role_bindings
            ADD CONSTRAINT fk_iam_role_bindings_role
            FOREIGN KEY (organization_id, role_id)
            REFERENCES iam_roles(org_id, id)
            ON DELETE CASCADE;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_api_tokens_iam_role'
    ) THEN
        ALTER TABLE api_tokens
            ADD CONSTRAINT fk_api_tokens_iam_role
            FOREIGN KEY (org_id, role_id)
            REFERENCES iam_roles(org_id, id)
            ON DELETE CASCADE;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_service_accounts_iam_role'
    ) THEN
        ALTER TABLE service_accounts
            ADD CONSTRAINT fk_service_accounts_iam_role
            FOREIGN KEY (org_id, role_id)
            REFERENCES iam_roles(org_id, id)
            ON DELETE CASCADE;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_api_tokens_service_account'
    ) THEN
        ALTER TABLE api_tokens
            ADD CONSTRAINT fk_api_tokens_service_account
            FOREIGN KEY (org_id, service_account_id)
            REFERENCES service_accounts(org_id, id)
            ON DELETE CASCADE;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_invitations_iam_role'
    ) THEN
        ALTER TABLE invitations
            ADD CONSTRAINT fk_invitations_iam_role
            FOREIGN KEY (org_id, role_id)
            REFERENCES iam_roles(org_id, id)
            ON DELETE RESTRICT;
    END IF;
END
$$;

CREATE TABLE IF NOT EXISTS iam_relationships (
    id                  VARCHAR(64) PRIMARY KEY,
    organization_id     VARCHAR(64) NOT NULL,
    resource_type       VARCHAR(64) NOT NULL,
    resource_id         VARCHAR(255) NOT NULL,
    role_id             VARCHAR(64) NOT NULL,
    subject_type        VARCHAR(24) NOT NULL,
    subject_id          VARCHAR(64) NOT NULL,
    container_type      VARCHAR(64),
    container_id        VARCHAR(255),
    created_by          VARCHAR(64) NOT NULL,
    created_at_micros   BIGINT      NOT NULL,
    CONSTRAINT fk_iam_relationship_role
        FOREIGN KEY (organization_id, role_id)
        REFERENCES iam_roles(org_id, id)
        ON DELETE RESTRICT,
    CONSTRAINT chk_iam_relationship_subject
        CHECK (subject_type IN ('user', 'team', 'group', 'service_account', 'organization')),
    CONSTRAINT chk_iam_relationship_container
        CHECK (
            (container_type IS NULL AND container_id IS NULL)
            OR (container_type IS NOT NULL AND container_id IS NOT NULL)
        )
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_iam_relationship
    ON iam_relationships (
        organization_id,
        resource_type,
        resource_id,
        role_id,
        subject_type,
        subject_id
    );
CREATE INDEX IF NOT EXISTS idx_iam_relationship_subject
    ON iam_relationships (organization_id, subject_type, subject_id);
CREATE INDEX IF NOT EXISTS idx_iam_relationship_resource
    ON iam_relationships (organization_id, resource_type, resource_id);

CREATE TABLE IF NOT EXISTS iam_cross_org_grants (
    id                      VARCHAR(64) PRIMARY KEY,
    source_organization_id  VARCHAR(64) NOT NULL,
    target_organization_id  VARCHAR(64) NOT NULL,
    grantee_type            VARCHAR(24) NOT NULL,
    grantee_id              VARCHAR(64) NOT NULL,
    resource_type           VARCHAR(64) NOT NULL,
    resource_selector       JSONB       NOT NULL,
    permissions             JSONB       NOT NULL,
    conditions              JSONB       NOT NULL DEFAULT '{}'::JSONB,
    starts_at_micros        BIGINT,
    expires_at_micros       BIGINT,
    status                  VARCHAR(16) NOT NULL DEFAULT 'pending',
    approved_by             VARCHAR(64),
    approved_at_micros      BIGINT,
    revoked_by              VARCHAR(64),
    revoked_at_micros       BIGINT,
    created_by              VARCHAR(64) NOT NULL,
    created_at_micros       BIGINT      NOT NULL,
    CONSTRAINT chk_iam_cross_org_distinct
        CHECK (source_organization_id <> target_organization_id),
    CONSTRAINT chk_iam_cross_org_grantee
        CHECK (grantee_type IN ('user', 'team', 'group', 'service_account', 'organization')),
    CONSTRAINT chk_iam_cross_org_status
        CHECK (status IN ('pending', 'active', 'revoked')),
    CONSTRAINT chk_iam_cross_org_window
        CHECK (
            starts_at_micros IS NULL
            OR expires_at_micros IS NULL
            OR starts_at_micros < expires_at_micros
        )
);
CREATE INDEX IF NOT EXISTS idx_iam_cross_org_grants_source
    ON iam_cross_org_grants (source_organization_id, status);
CREATE INDEX IF NOT EXISTS idx_iam_cross_org_grants_target
    ON iam_cross_org_grants (target_organization_id, grantee_type, grantee_id, status);

-- Built-in role catalog rows are needed before membership bindings can be
-- materialized. Permission rows are reconciled from the IAM catalog by
-- PgIamRoleRepository::ensure_builtin_roles.
INSERT INTO iam_roles (
    id,
    org_id,
    role_key,
    name,
    description,
    builtin,
    role_type,
    scope,
    created_at_micros,
    updated_at_micros
)
SELECT
    gen_random_uuid()::TEXT,
    org.id,
    role.role_key,
    role.name,
    role.description,
    TRUE,
    role.role_type,
    role.scope,
    (EXTRACT(EPOCH FROM clock_timestamp()) * 1000000)::BIGINT,
    (EXTRACT(EPOCH FROM clock_timestamp()) * 1000000)::BIGINT
FROM organizations org
CROSS JOIN iam_builtin_roles role
WHERE (
        role.role_type = 'organization'
        AND role.scope = 'organization'
    )
   OR (
        org.system
        AND role.role_type = 'platform'
        AND role.scope = 'platform'
    )
ON CONFLICT (org_id, role_key) DO NOTHING;

-- Convert role keys into stable IAM role IDs. Unknown keys abort schema
-- initialization instead of silently weakening access.
DO $$
DECLARE
    unmapped BIGINT;
BEGIN
    IF EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'iam_memberships'
          AND column_name = 'role'
    ) THEN
        INSERT INTO iam_role_bindings (
            id,
            organization_id,
            role_id,
            principal_type,
            principal_id,
            resource_type,
            resource_id,
            conditions,
            starts_at_micros,
            expires_at_micros,
            created_by,
            created_at_micros
        )
        SELECT
            'migrated_' || substr(
                md5(
                    membership.org_id || ':' ||
                    membership.user_id || ':' ||
                    lower(membership.role)
                ),
                1,
                32
            ),
            membership.org_id,
            role.id,
            'user',
            membership.user_id,
            NULL,
            NULL,
            '{}'::JSONB,
            NULL,
            NULL,
            membership.user_id,
            membership.joined_at_micros
        FROM iam_memberships membership
        JOIN iam_roles role
          ON role.org_id = membership.org_id
         AND role.role_key = lower(membership.role)
        WHERE NOT EXISTS (
            SELECT 1
            FROM iam_role_bindings binding
            WHERE binding.organization_id = membership.org_id
              AND binding.principal_type = 'user'
              AND binding.principal_id = membership.user_id
              AND binding.role_id = role.id
              AND binding.resource_type IS NULL
              AND binding.resource_id IS NULL
        )
        ON CONFLICT (id) DO NOTHING;

        SELECT COUNT(*) INTO unmapped
        FROM iam_memberships membership
        LEFT JOIN iam_roles role
          ON role.org_id = membership.org_id
         AND role.role_key = lower(membership.role)
        WHERE role.id IS NULL;
        IF unmapped > 0 THEN
            RAISE EXCEPTION
                'cannot migrate % IAM membership role value(s)', unmapped;
        END IF;

        ALTER TABLE iam_memberships DROP COLUMN role;
    END IF;

    IF EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'api_tokens'
          AND column_name = 'role'
    ) THEN
        UPDATE api_tokens token
        SET role_id = role.id
        FROM iam_roles role
        WHERE token.role_id IS NULL
          AND role.org_id = token.org_id
          AND role.role_key = lower(token.role);

        SELECT COUNT(*) INTO unmapped
        FROM api_tokens
        WHERE role_id IS NULL;
        IF unmapped > 0 THEN
            RAISE EXCEPTION
                'cannot migrate % API token role value(s)', unmapped;
        END IF;

        ALTER TABLE api_tokens DROP COLUMN role;
    END IF;
    ALTER TABLE api_tokens ALTER COLUMN role_id SET NOT NULL;

    IF EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'invitations'
          AND column_name = 'role'
    ) THEN
        UPDATE invitations invitation
        SET role_id = role.id
        FROM iam_roles role
        WHERE invitation.role_id IS NULL
          AND role.org_id = invitation.org_id
          AND role.role_key = lower(invitation.role);

        SELECT COUNT(*) INTO unmapped
        FROM invitations
        WHERE role_id IS NULL;
        IF unmapped > 0 THEN
            RAISE EXCEPTION
                'cannot migrate % invitation role value(s)', unmapped;
        END IF;

        ALTER TABLE invitations DROP COLUMN role;
    END IF;
    ALTER TABLE invitations ALTER COLUMN role_id SET NOT NULL;
END
$$;

CREATE OR REPLACE FUNCTION bump_iam_policy_version(p_organization_id VARCHAR)
RETURNS BIGINT LANGUAGE plpgsql AS $$
DECLARE
    next_version BIGINT;
BEGIN
    INSERT INTO iam_policy_versions (organization_id, version, updated_at_micros)
    VALUES (
        p_organization_id,
        1,
        (EXTRACT(EPOCH FROM clock_timestamp()) * 1000000)::BIGINT
    )
    ON CONFLICT (organization_id) DO UPDATE
       SET version = iam_policy_versions.version + 1,
           updated_at_micros =
               (EXTRACT(EPOCH FROM clock_timestamp()) * 1000000)::BIGINT
    RETURNING version INTO next_version;
    RETURN next_version;
END
$$;

DROP TABLE IF EXISTS rbac_policies;

-- ============================================================
-- IAM permission catalog
-- ============================================================

-- IAM permission catalog.
--
-- Runtime IAM evaluation, role editing, and frontend capability metadata all
-- read these normalized tables. These rows are the bootstrap seed; there is no
-- file-based runtime registry.

CREATE TABLE IF NOT EXISTS iam_permission_catalog_versions (
    catalog_key         VARCHAR(64) PRIMARY KEY,
    version             BIGINT      NOT NULL CHECK (version > 0),
    updated_at_micros   BIGINT      NOT NULL
);

CREATE TABLE IF NOT EXISTS iam_permissions (
    permission_key      VARCHAR(128) PRIMARY KEY,
    scope               VARCHAR(16)  NOT NULL,
    domain              VARCHAR(64)  NOT NULL,
    label_key           VARCHAR(160) NOT NULL,
    description_key     VARCHAR(160) NOT NULL,
    feature             VARCHAR(64),
    catalog_version     BIGINT       NOT NULL,
    CONSTRAINT chk_iam_permissions_scope
        CHECK (scope IN ('platform', 'organization'))
);
CREATE INDEX IF NOT EXISTS idx_iam_permissions_scope_domain
    ON iam_permissions (scope, domain, permission_key);

CREATE TABLE IF NOT EXISTS iam_builtin_role_permissions (
    role_key            VARCHAR(64)  NOT NULL
        REFERENCES iam_builtin_roles(role_key) ON DELETE CASCADE,
    permission_key      VARCHAR(128) NOT NULL
        REFERENCES iam_permissions(permission_key) ON DELETE CASCADE,
    PRIMARY KEY (role_key, permission_key)
);

CREATE TABLE IF NOT EXISTS iam_permission_bundles (
    bundle_key          VARCHAR(64)  PRIMARY KEY,
    label_key           VARCHAR(160) NOT NULL,
    description_key     VARCHAR(160) NOT NULL,
    catalog_version     BIGINT       NOT NULL
);

CREATE TABLE IF NOT EXISTS iam_permission_bundle_items (
    bundle_key          VARCHAR(64)  NOT NULL
        REFERENCES iam_permission_bundles(bundle_key) ON DELETE CASCADE,
    permission_key      VARCHAR(128) NOT NULL
        REFERENCES iam_permissions(permission_key) ON DELETE CASCADE,
    position            INTEGER      NOT NULL CHECK (position >= 0),
    PRIMARY KEY (bundle_key, permission_key)
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_iam_permission_bundle_position
    ON iam_permission_bundle_items (bundle_key, position);

INSERT INTO iam_permission_catalog_versions (catalog_key, version, updated_at_micros)
VALUES (
    'permissions',
    5,
    (EXTRACT(EPOCH FROM clock_timestamp()) * 1000000)::BIGINT
)
ON CONFLICT (catalog_key) DO UPDATE
SET version = EXCLUDED.version,
    updated_at_micros = EXCLUDED.updated_at_micros;

CREATE TEMP TABLE iam_permission_seed (
    permission_key      VARCHAR(128) NOT NULL,
    scope               VARCHAR(16)  NOT NULL,
    domain              VARCHAR(64)  NOT NULL,
    label_key           VARCHAR(160) NOT NULL,
    description_key     VARCHAR(160) NOT NULL,
    feature             VARCHAR(64),
    builtin_roles       TEXT[]       NOT NULL
) ON COMMIT DROP;

INSERT INTO iam_permission_seed (
    permission_key,
    scope,
    domain,
    label_key,
    description_key,
    feature,
    builtin_roles
)
VALUES
    ('sys.organizations.manage', 'platform', 'platform', 'permissions.sys_organizations_manage', 'permissions_hint.sys_organizations_manage', NULL, ARRAY['platform_owner']::TEXT[]),
    ('sys.licenses.read', 'platform', 'platform', 'permissions.sys_licenses_read', 'permissions_hint.sys_licenses_read', NULL, ARRAY['platform_owner']::TEXT[]),
    ('sys.licenses.manage', 'platform', 'platform', 'permissions.sys_licenses_manage', 'permissions_hint.sys_licenses_manage', NULL, ARRAY['platform_owner']::TEXT[]),
    ('sys.settings.manage', 'platform', 'platform', 'permissions.sys_settings_manage', 'permissions_hint.sys_settings_manage', NULL, ARRAY['platform_owner']::TEXT[]),
    ('sys.telemetry.read', 'platform', 'platform', 'permissions.sys_telemetry_read', 'permissions_hint.sys_telemetry_read', NULL, ARRAY['platform_owner']::TEXT[]),
    ('sys.telemetry.manage', 'platform', 'platform', 'permissions.sys_telemetry_manage', 'permissions_hint.sys_telemetry_manage', NULL, ARRAY['platform_owner']::TEXT[]),
    ('sys.administrators.manage', 'platform', 'platform', 'permissions.sys_administrators_manage', 'permissions_hint.sys_administrators_manage', NULL, ARRAY['platform_owner']::TEXT[]),
    ('sys.trace_debug.manage', 'platform', 'platform', 'permissions.sys_trace_debug_manage', 'permissions_hint.sys_trace_debug_manage', NULL, ARRAY['platform_owner']::TEXT[]),
    ('org.settings.read', 'organization', 'organization', 'permissions.org_settings_read', 'permissions_hint.org_settings_read', NULL, ARRAY['owner', 'admin']::TEXT[]),
    ('org.settings.manage', 'organization', 'organization', 'permissions.org_settings_manage', 'permissions_hint.org_settings_manage', NULL, ARRAY['owner', 'admin']::TEXT[]),
    ('org.members.read', 'organization', 'iam', 'permissions.org_members_read', 'permissions_hint.org_members_read', NULL, ARRAY['owner', 'admin']::TEXT[]),
    ('org.members.manage', 'organization', 'iam', 'permissions.org_members_manage', 'permissions_hint.org_members_manage', NULL, ARRAY['owner', 'admin']::TEXT[]),
    ('iam.roles.read', 'organization', 'iam', 'permissions.iam_roles_read', 'permissions_hint.iam_roles_read', NULL, ARRAY['owner', 'admin']::TEXT[]),
    ('iam.roles.manage', 'organization', 'iam', 'permissions.iam_roles_manage', 'permissions_hint.iam_roles_manage', NULL, ARRAY['owner', 'admin']::TEXT[]),
    ('iam.policies.read', 'organization', 'iam', 'permissions.iam_policies_read', 'permissions_hint.iam_policies_read', NULL, ARRAY['owner', 'admin']::TEXT[]),
    ('iam.policies.manage', 'organization', 'iam', 'permissions.iam_policies_manage', 'permissions_hint.iam_policies_manage', NULL, ARRAY['owner', 'admin']::TEXT[]),
    ('org.billing.read', 'organization', 'organization', 'permissions.org_billing_read', 'permissions_hint.org_billing_read', NULL, ARRAY['owner', 'admin']::TEXT[]),
    ('org.billing.manage', 'organization', 'organization', 'permissions.org_billing_manage', 'permissions_hint.org_billing_manage', NULL, ARRAY['owner', 'admin']::TEXT[]),
    ('api_tokens.read', 'organization', 'iam', 'permissions.api_tokens_read', 'permissions_hint.api_tokens_read', NULL, ARRAY['owner', 'admin']::TEXT[]),
    ('api_tokens.manage', 'organization', 'iam', 'permissions.api_tokens_manage', 'permissions_hint.api_tokens_manage', NULL, ARRAY['owner', 'admin']::TEXT[]),
    ('service_accounts.read', 'organization', 'iam', 'permissions.service_accounts_read', 'permissions_hint.service_accounts_read', NULL, ARRAY['owner', 'admin']::TEXT[]),
    ('service_accounts.manage', 'organization', 'iam', 'permissions.service_accounts_manage', 'permissions_hint.service_accounts_manage', NULL, ARRAY['owner', 'admin']::TEXT[]),
    ('streams.read', 'organization', 'observability', 'permissions.streams_read', 'permissions_hint.streams_read', NULL, ARRAY['owner', 'admin', 'editor', 'viewer']::TEXT[]),
    ('streams.query', 'organization', 'observability', 'permissions.streams_query', 'permissions_hint.streams_query', NULL, ARRAY['owner', 'admin', 'editor', 'viewer']::TEXT[]),
    ('streams.write', 'organization', 'observability', 'permissions.streams_write', 'permissions_hint.streams_write', NULL, ARRAY['owner', 'admin', 'editor', 'intake']::TEXT[]),
    ('rum.write', 'organization', 'observability', 'permissions.rum_write', 'permissions_hint.rum_write', NULL, ARRAY['owner', 'admin', 'editor', 'rum_client']::TEXT[]),
    ('streams.create', 'organization', 'observability', 'permissions.streams_create', 'permissions_hint.streams_create', NULL, ARRAY['owner', 'admin', 'editor']::TEXT[]),
    ('streams.configure', 'organization', 'observability', 'permissions.streams_configure', 'permissions_hint.streams_configure', NULL, ARRAY['owner', 'admin', 'editor']::TEXT[]),
    ('streams.delete', 'organization', 'observability', 'permissions.streams_delete', 'permissions_hint.streams_delete', NULL, ARRAY['owner', 'admin', 'editor']::TEXT[]),
    ('dashboards.read', 'organization', 'dashboards', 'permissions.dashboards_read', 'permissions_hint.dashboards_read', NULL, ARRAY['owner', 'admin', 'editor', 'viewer']::TEXT[]),
    ('dashboards.edit', 'organization', 'dashboards', 'permissions.dashboards_edit', 'permissions_hint.dashboards_edit', NULL, ARRAY['owner', 'admin', 'editor']::TEXT[]),
    ('dashboards.create', 'organization', 'dashboards', 'permissions.dashboards_create', 'permissions_hint.dashboards_create', NULL, ARRAY['owner', 'admin', 'editor']::TEXT[]),
    ('dashboards.delete', 'organization', 'dashboards', 'permissions.dashboards_delete', 'permissions_hint.dashboards_delete', NULL, ARRAY['owner', 'admin', 'editor']::TEXT[]),
    ('dashboards.share', 'organization', 'dashboards', 'permissions.dashboards_share', 'permissions_hint.dashboards_share', NULL, ARRAY['owner', 'admin', 'editor']::TEXT[]),
    ('alerts.read', 'organization', 'alerts', 'permissions.alerts_read', 'permissions_hint.alerts_read', NULL, ARRAY['owner', 'admin', 'editor', 'viewer']::TEXT[]),
    ('alerts.manage', 'organization', 'alerts', 'permissions.alerts_manage', 'permissions_hint.alerts_manage', NULL, ARRAY['owner', 'admin', 'editor']::TEXT[]),
    ('alerts.acknowledge', 'organization', 'alerts', 'permissions.alerts_acknowledge', 'permissions_hint.alerts_acknowledge', NULL, ARRAY['owner', 'admin', 'editor']::TEXT[]),
    ('alerts.silence', 'organization', 'alerts', 'permissions.alerts_silence', 'permissions_hint.alerts_silence', NULL, ARRAY['owner', 'admin', 'editor']::TEXT[]),
    ('schedules.read', 'organization', 'alerts', 'permissions.schedules_read', 'permissions_hint.schedules_read', NULL, ARRAY['owner', 'admin', 'editor', 'viewer']::TEXT[]),
    ('schedules.manage', 'organization', 'alerts', 'permissions.schedules_manage', 'permissions_hint.schedules_manage', NULL, ARRAY['owner', 'admin', 'editor']::TEXT[]),
    ('saved_views.read', 'organization', 'observability', 'permissions.saved_views_read', 'permissions_hint.saved_views_read', NULL, ARRAY['owner', 'admin', 'editor', 'viewer']::TEXT[]),
    ('saved_views.create', 'organization', 'observability', 'permissions.saved_views_create', 'permissions_hint.saved_views_create', NULL, ARRAY['owner', 'admin', 'editor']::TEXT[]),
    ('saved_views.edit', 'organization', 'observability', 'permissions.saved_views_edit', 'permissions_hint.saved_views_edit', NULL, ARRAY['owner', 'admin', 'editor']::TEXT[]),
    ('saved_views.delete', 'organization', 'observability', 'permissions.saved_views_delete', 'permissions_hint.saved_views_delete', NULL, ARRAY['owner', 'admin', 'editor']::TEXT[]),
    ('pipelines.read', 'organization', 'pipelines', 'permissions.pipelines_read', 'permissions_hint.pipelines_read', NULL, ARRAY['owner', 'admin', 'editor', 'viewer']::TEXT[]),
    ('pipelines.create', 'organization', 'pipelines', 'permissions.pipelines_create', 'permissions_hint.pipelines_create', NULL, ARRAY['owner', 'admin', 'editor']::TEXT[]),
    ('pipelines.edit', 'organization', 'pipelines', 'permissions.pipelines_edit', 'permissions_hint.pipelines_edit', NULL, ARRAY['owner', 'admin', 'editor']::TEXT[]),
    ('pipelines.run', 'organization', 'pipelines', 'permissions.pipelines_run', 'permissions_hint.pipelines_run', NULL, ARRAY['owner', 'admin', 'editor']::TEXT[]),
    ('pipelines.pause', 'organization', 'pipelines', 'permissions.pipelines_pause', 'permissions_hint.pipelines_pause', NULL, ARRAY['owner', 'admin', 'editor']::TEXT[]),
    ('pipelines.delete', 'organization', 'pipelines', 'permissions.pipelines_delete', 'permissions_hint.pipelines_delete', NULL, ARRAY['owner', 'admin', 'editor']::TEXT[]),
    ('functions.read', 'organization', 'pipelines', 'permissions.functions_read', 'permissions_hint.functions_read', NULL, ARRAY['owner', 'admin', 'editor', 'viewer']::TEXT[]),
    ('functions.create', 'organization', 'pipelines', 'permissions.functions_create', 'permissions_hint.functions_create', NULL, ARRAY['owner', 'admin', 'editor']::TEXT[]),
    ('functions.edit', 'organization', 'pipelines', 'permissions.functions_edit', 'permissions_hint.functions_edit', NULL, ARRAY['owner', 'admin', 'editor']::TEXT[]),
    ('functions.run', 'organization', 'pipelines', 'permissions.functions_run', 'permissions_hint.functions_run', NULL, ARRAY['owner', 'admin', 'editor']::TEXT[]),
    ('functions.delete', 'organization', 'pipelines', 'permissions.functions_delete', 'permissions_hint.functions_delete', NULL, ARRAY['owner', 'admin', 'editor']::TEXT[]),
    ('reports.read', 'organization', 'reports', 'permissions.reports_read', 'permissions_hint.reports_read', NULL, ARRAY['owner', 'admin', 'editor', 'viewer']::TEXT[]),
    ('reports.create', 'organization', 'reports', 'permissions.reports_create', 'permissions_hint.reports_create', NULL, ARRAY['owner', 'admin', 'editor']::TEXT[]),
    ('reports.edit', 'organization', 'reports', 'permissions.reports_edit', 'permissions_hint.reports_edit', NULL, ARRAY['owner', 'admin', 'editor']::TEXT[]),
    ('reports.schedule', 'organization', 'reports', 'permissions.reports_schedule', 'permissions_hint.reports_schedule', NULL, ARRAY['owner', 'admin', 'editor']::TEXT[]),
    ('reports.delete', 'organization', 'reports', 'permissions.reports_delete', 'permissions_hint.reports_delete', NULL, ARRAY['owner', 'admin', 'editor']::TEXT[]),
    ('audit.read', 'organization', 'iam', 'permissions.audit_read', 'permissions_hint.audit_read', NULL, ARRAY['owner', 'admin']::TEXT[]),
    ('agent.use', 'organization', 'agent', 'permissions.agent_use', 'permissions_hint.agent_use', 'agent', ARRAY['owner', 'admin', 'editor', 'viewer']::TEXT[]),
    ('agent.manage', 'organization', 'agent', 'permissions.agent_manage', 'permissions_hint.agent_manage', 'agent', ARRAY['owner', 'admin']::TEXT[]),
    ('agent.approve', 'organization', 'agent', 'permissions.agent_approve', 'permissions_hint.agent_approve', 'agent', ARRAY['owner', 'admin']::TEXT[]);

INSERT INTO iam_permissions (
    permission_key,
    scope,
    domain,
    label_key,
    description_key,
    feature,
    catalog_version
)
SELECT
    permission_key,
    scope,
    domain,
    label_key,
    description_key,
    feature,
    CASE WHEN permission_key = 'rum.write' THEN 7 ELSE 5 END
FROM iam_permission_seed
ON CONFLICT (permission_key) DO UPDATE
SET scope = EXCLUDED.scope,
    domain = EXCLUDED.domain,
    label_key = EXCLUDED.label_key,
    description_key = EXCLUDED.description_key,
    feature = EXCLUDED.feature,
    catalog_version = EXCLUDED.catalog_version;

-- Replace provisional built-in permission assignments before enforcing the
-- canonical catalog foreign key. Built-in roles are populated from
-- iam_permission_seed below; custom assignments must use catalog keys rather
-- than being silently broadened.
DELETE FROM iam_role_permissions role_permission
USING iam_roles role
WHERE role.id = role_permission.role_id
  AND role.builtin;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
          FROM pg_constraint
         WHERE conname = 'fk_iam_role_permissions_catalog'
    ) THEN
        ALTER TABLE iam_role_permissions
            ADD CONSTRAINT fk_iam_role_permissions_catalog
            FOREIGN KEY (permission_key)
            REFERENCES iam_permissions(permission_key)
            ON DELETE RESTRICT;
    END IF;
END
$$;

DELETE FROM iam_builtin_role_permissions;
INSERT INTO iam_builtin_role_permissions (role_key, permission_key)
SELECT role_key, seed.permission_key
FROM iam_permission_seed seed
CROSS JOIN LATERAL unnest(seed.builtin_roles) AS role_key;

CREATE TEMP TABLE iam_permission_bundle_seed (
    bundle_key          VARCHAR(64)  NOT NULL,
    label_key           VARCHAR(160) NOT NULL,
    description_key     VARCHAR(160) NOT NULL,
    permissions         TEXT[]       NOT NULL
) ON COMMIT DROP;

INSERT INTO iam_permission_bundle_seed (
    bundle_key,
    label_key,
    description_key,
    permissions
)
VALUES
    ('readonly_observer', 'roles.bundles.readonly_observer', 'roles.bundles_hint.readonly_observer', ARRAY['streams.read', 'streams.query', 'dashboards.read', 'alerts.read', 'schedules.read', 'saved_views.read', 'pipelines.read', 'functions.read', 'reports.read']::TEXT[]),
    ('data_analyst', 'roles.bundles.data_analyst', 'roles.bundles_hint.data_analyst', ARRAY['streams.read', 'streams.query', 'dashboards.read', 'dashboards.create', 'dashboards.edit', 'saved_views.read', 'saved_views.create', 'saved_views.edit', 'reports.read']::TEXT[]),
    ('pipeline_developer', 'roles.bundles.pipeline_developer', 'roles.bundles_hint.pipeline_developer', ARRAY['streams.read', 'streams.query', 'streams.write', 'streams.create', 'streams.configure', 'pipelines.read', 'pipelines.create', 'pipelines.edit', 'pipelines.run', 'pipelines.pause', 'pipelines.delete', 'functions.read', 'functions.create', 'functions.edit', 'functions.run', 'functions.delete']::TEXT[]),
    ('alert_administrator', 'roles.bundles.alert_administrator', 'roles.bundles_hint.alert_administrator', ARRAY['streams.read', 'streams.query', 'alerts.read', 'alerts.manage', 'alerts.acknowledge', 'alerts.silence', 'schedules.read', 'schedules.manage']::TEXT[]),
    ('organization_administrator', 'roles.bundles.organization_administrator', 'roles.bundles_hint.organization_administrator', ARRAY['org.settings.read', 'org.settings.manage', 'org.members.read', 'org.members.manage', 'iam.roles.read', 'iam.roles.manage', 'iam.policies.read', 'iam.policies.manage', 'api_tokens.read', 'api_tokens.manage', 'service_accounts.read', 'service_accounts.manage', 'audit.read']::TEXT[]);

INSERT INTO iam_permission_bundles (
    bundle_key,
    label_key,
    description_key,
    catalog_version
)
SELECT bundle_key, label_key, description_key, 5
FROM iam_permission_bundle_seed
ON CONFLICT (bundle_key) DO UPDATE
SET label_key = EXCLUDED.label_key,
    description_key = EXCLUDED.description_key,
    catalog_version = EXCLUDED.catalog_version;

DELETE FROM iam_permission_bundle_items;
INSERT INTO iam_permission_bundle_items (bundle_key, permission_key, position)
SELECT
    seed.bundle_key,
    item.permission_key,
    item.position - 1
FROM iam_permission_bundle_seed seed
CROSS JOIN LATERAL unnest(seed.permissions)
    WITH ORDINALITY AS item(permission_key, position);

INSERT INTO iam_role_permissions (role_id, permission_key)
SELECT role.id, builtin.permission_key
FROM iam_roles role
JOIN iam_builtin_role_permissions builtin
  ON builtin.role_key = role.role_key
WHERE role.builtin
ON CONFLICT (role_id, permission_key) DO NOTHING;

-- ============================================================
-- Resource-scoped shares
-- ============================================================

-- Resource-scoped sharing.
--
-- A resource share is an auditable, revocable principal with a bounded
-- permission set. Plaintext share/session tokens are never persisted.
--
CREATE TABLE IF NOT EXISTS resource_shares (
    id                         VARCHAR(64) PRIMARY KEY,
    organization_id            VARCHAR(64) NOT NULL,
    resource_type              VARCHAR(32) NOT NULL,
    resource_id                VARCHAR(64) NOT NULL,
    resource_version_id        VARCHAR(64),
    share_mode                 VARCHAR(32) NOT NULL,
    token_hash                 VARCHAR(64) NOT NULL UNIQUE,
    permissions_json           JSONB NOT NULL DEFAULT '[]'::JSONB,
    constraints_json           JSONB NOT NULL DEFAULT '{}'::JSONB,
    expires_at_micros          BIGINT,
    password_hash              TEXT,
    max_views                  BIGINT,
    view_count                 BIGINT NOT NULL DEFAULT 0,
    allow_download             BOOLEAN NOT NULL DEFAULT FALSE,
    enabled                    BOOLEAN NOT NULL DEFAULT TRUE,
    cross_org_grant_id         VARCHAR(64),
    snapshot_object_key        TEXT,
    snapshot_content_type      VARCHAR(128),
    snapshot_filename          VARCHAR(255),
    created_by                 VARCHAR(64) NOT NULL,
    created_at_micros          BIGINT NOT NULL,
    last_accessed_at_micros    BIGINT,
    revoked_at_micros          BIGINT,
    CONSTRAINT chk_resource_share_type
        CHECK (resource_type IN ('dashboard', 'report', 'report_file')),
    CONSTRAINT chk_resource_share_mode
        CHECK (share_mode IN ('authenticated', 'cross_org', 'public_link')),
    CONSTRAINT chk_resource_share_max_views
        CHECK (max_views IS NULL OR max_views > 0),
    CONSTRAINT chk_resource_share_view_count
        CHECK (view_count >= 0)
);
CREATE INDEX IF NOT EXISTS idx_resource_shares_org_resource
    ON resource_shares (organization_id, resource_type, resource_id, created_at_micros DESC);
CREATE INDEX IF NOT EXISTS idx_resource_shares_expiry
    ON resource_shares (expires_at_micros)
    WHERE enabled;

CREATE TABLE IF NOT EXISTS resource_share_sessions (
    id                         VARCHAR(64) PRIMARY KEY,
    share_id                   VARCHAR(64) NOT NULL
        REFERENCES resource_shares(id) ON DELETE CASCADE,
    session_token_hash         VARCHAR(64) NOT NULL UNIQUE,
    password_verified          BOOLEAN NOT NULL DEFAULT FALSE,
    created_at_micros          BIGINT NOT NULL,
    expires_at_micros          BIGINT NOT NULL,
    last_seen_at_micros        BIGINT NOT NULL,
    ip                         VARCHAR(128),
    user_agent                 TEXT
);
CREATE INDEX IF NOT EXISTS idx_resource_share_sessions_expiry
    ON resource_share_sessions (expires_at_micros);

CREATE TABLE IF NOT EXISTS resource_share_policies (
    organization_id                    VARCHAR(64) PRIMARY KEY,
    allow_public_links                 BOOLEAN NOT NULL DEFAULT FALSE,
    allow_public_dashboards            BOOLEAN NOT NULL DEFAULT FALSE,
    max_public_expiry_secs             BIGINT NOT NULL DEFAULT 604800,
    require_public_report_password     BOOLEAN NOT NULL DEFAULT TRUE,
    deny_production_public_shares      BOOLEAN NOT NULL DEFAULT TRUE,
    allow_public_csv_download          BOOLEAN NOT NULL DEFAULT FALSE,
    updated_by                         VARCHAR(64) NOT NULL,
    updated_at_micros                  BIGINT NOT NULL,
    CONSTRAINT chk_resource_share_policy_expiry
        CHECK (max_public_expiry_secs BETWEEN 3600 AND 2592000)
);

-- Report sharing is a distinct capability from reading reports.
INSERT INTO iam_permissions (
    permission_key,
    scope,
    domain,
    label_key,
    description_key,
    feature,
    catalog_version
)
VALUES (
    'reports.share',
    'organization',
    'reports',
    'permissions.reports_share',
    'permissions_hint.reports_share',
    NULL,
    5
)
ON CONFLICT (permission_key) DO UPDATE
SET scope = EXCLUDED.scope,
    domain = EXCLUDED.domain,
    label_key = EXCLUDED.label_key,
    description_key = EXCLUDED.description_key,
    feature = EXCLUDED.feature,
    catalog_version = EXCLUDED.catalog_version;

INSERT INTO iam_builtin_role_permissions (role_key, permission_key)
VALUES
    ('owner', 'reports.share'),
    ('admin', 'reports.share'),
    ('editor', 'reports.share')
ON CONFLICT (role_key, permission_key) DO NOTHING;

INSERT INTO iam_role_permissions (role_id, permission_key)
SELECT role.id, 'reports.share'
FROM iam_roles role
WHERE role.builtin
  AND role.role_key IN ('owner', 'admin', 'editor')
ON CONFLICT (role_id, permission_key) DO NOTHING;

INSERT INTO iam_permission_catalog_versions (
    catalog_key,
    version,
    updated_at_micros
)
VALUES (
    'permissions',
    5,
    (EXTRACT(EPOCH FROM clock_timestamp()) * 1000000)::BIGINT
)
ON CONFLICT (catalog_key) DO UPDATE
SET version = GREATEST(iam_permission_catalog_versions.version, EXCLUDED.version),
    updated_at_micros = EXCLUDED.updated_at_micros;

-- `_sys`, its platform role, and its bundled dashboards are database-owned
-- seed data. Keep the stable organization identifier and role materialization
-- here so application bootstrap only has to reconcile this state idempotently.
INSERT INTO organizations (
    id,
    name,
    slug,
    created_at_micros,
    system
)
VALUES (
    'system_' || substr(md5('molesignal:_sys'), 1, 32),
    '_sys',
    '_sys',
    (EXTRACT(EPOCH FROM clock_timestamp()) * 1000000)::BIGINT,
    TRUE
)
ON CONFLICT (slug) DO NOTHING;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
          FROM organizations
         WHERE name = '_sys'
           AND slug = '_sys'
           AND system
    ) THEN
        RAISE EXCEPTION 'invalid or conflicting `_sys` organization seed';
    END IF;
END
$$;

INSERT INTO iam_policy_versions (organization_id, version, updated_at_micros)
SELECT
    organization.id,
    1,
    (EXTRACT(EPOCH FROM clock_timestamp()) * 1000000)::BIGINT
FROM organizations organization
WHERE organization.slug = '_sys'
  AND organization.system
ON CONFLICT (organization_id) DO NOTHING;

INSERT INTO iam_roles (
    id,
    org_id,
    role_key,
    name,
    description,
    builtin,
    role_type,
    scope,
    created_at_micros,
    updated_at_micros
)
SELECT
    gen_random_uuid()::TEXT,
    organization.id,
    role.role_key,
    role.name,
    role.description,
    TRUE,
    role.role_type,
    role.scope,
    (EXTRACT(EPOCH FROM clock_timestamp()) * 1000000)::BIGINT,
    (EXTRACT(EPOCH FROM clock_timestamp()) * 1000000)::BIGINT
FROM organizations organization
JOIN iam_builtin_roles role
  ON role.role_type = 'platform'
 AND role.scope = 'platform'
WHERE organization.slug = '_sys'
  AND organization.system
ON CONFLICT (org_id, role_key) DO UPDATE
SET name = EXCLUDED.name,
    description = EXCLUDED.description,
    builtin = TRUE,
    role_type = EXCLUDED.role_type,
    scope = EXCLUDED.scope,
    updated_at_micros = EXCLUDED.updated_at_micros
WHERE iam_roles.name IS DISTINCT FROM EXCLUDED.name
   OR iam_roles.description IS DISTINCT FROM EXCLUDED.description
   OR iam_roles.builtin IS DISTINCT FROM TRUE
   OR iam_roles.role_type IS DISTINCT FROM EXCLUDED.role_type
   OR iam_roles.scope IS DISTINCT FROM EXCLUDED.scope;

-- The system organization owns read-only built-in dashboards. Keep this
-- capability platform-scoped so `_sys` never needs an organization membership
-- or an organization-scoped dashboard role.
INSERT INTO iam_permissions (
    permission_key,
    scope,
    domain,
    label_key,
    description_key,
    feature,
    catalog_version
)
VALUES (
    'sys.dashboards.read',
    'platform',
    'platform',
    'permissions.sys_dashboards_read',
    'permissions_hint.sys_dashboards_read',
    NULL,
    6
)
ON CONFLICT (permission_key) DO UPDATE
SET scope = EXCLUDED.scope,
    domain = EXCLUDED.domain,
    label_key = EXCLUDED.label_key,
    description_key = EXCLUDED.description_key,
    feature = EXCLUDED.feature,
    catalog_version = EXCLUDED.catalog_version;

INSERT INTO iam_builtin_role_permissions (role_key, permission_key)
VALUES ('platform_owner', 'sys.dashboards.read')
ON CONFLICT (role_key, permission_key) DO NOTHING;

INSERT INTO iam_role_permissions (role_id, permission_key)
SELECT role.id, builtin.permission_key
FROM iam_roles role
JOIN organizations organization
  ON organization.id = role.org_id
 AND organization.system
JOIN iam_builtin_role_permissions builtin
  ON builtin.role_key = role.role_key
WHERE role.builtin
  AND role.role_key = 'platform_owner'
  AND role.scope = 'platform'
ON CONFLICT (role_id, permission_key) DO NOTHING;

INSERT INTO iam_permission_catalog_versions (
    catalog_key,
    version,
    updated_at_micros
)
VALUES (
    'permissions',
    7,
    (EXTRACT(EPOCH FROM clock_timestamp()) * 1000000)::BIGINT
)
ON CONFLICT (catalog_key) DO UPDATE
SET version = GREATEST(iam_permission_catalog_versions.version, EXCLUDED.version),
    updated_at_micros = EXCLUDED.updated_at_micros;

-- Built-in _sys dashboards are maintained by
-- 20260101000002_builtin_dashboards.sql.

-- ============================================================
-- Resource share token envelopes
-- ============================================================

-- Repeatable resource-share link display.
--
-- The lookup digest remains the authorization index. The reversible token is
-- stored only as an AES-GCM envelope under MS_CIPHER_KEY so authorized users
-- can copy an existing link again without persisting plaintext credentials.
ALTER TABLE resource_shares
    ADD COLUMN IF NOT EXISTS token_ciphertext BYTEA,
    ADD COLUMN IF NOT EXISTS token_nonce BYTEA;

ALTER TABLE resource_shares
    ADD CONSTRAINT chk_resource_share_token_envelope
    CHECK (
        (token_ciphertext IS NULL AND token_nonce IS NULL)
        OR (token_ciphertext IS NOT NULL AND token_nonce IS NOT NULL)
    );

COMMENT ON COLUMN resource_shares.token_ciphertext IS
    'AES-GCM encrypted share token; never returned without share-management authorization';
COMMENT ON COLUMN resource_shares.token_nonce IS
    'AES-GCM nonce paired with token_ciphertext';

-- ============================================================
-- Notify management
-- ============================================================

CREATE TABLE IF NOT EXISTS notify_connectors (
    id                      VARCHAR(64)  PRIMARY KEY,
    organization_id         VARCHAR(64)  NOT NULL,
    name                    VARCHAR(255) NOT NULL,
    connector_type          VARCHAR(64)  NOT NULL,
    config_ciphertext       BYTEA        NOT NULL,
    config_nonce            BYTEA        NOT NULL,
    capabilities            JSONB        NOT NULL DEFAULT '{}'::JSONB,
    enabled                 BOOLEAN      NOT NULL DEFAULT TRUE,
    status                  VARCHAR(16)  NOT NULL DEFAULT 'unknown',
    last_tested_at_micros   BIGINT,
    last_test_status        VARCHAR(16),
    last_test_error         TEXT,
    legacy_channel_id       VARCHAR(64),
    created_at_micros       BIGINT       NOT NULL,
    updated_at_micros       BIGINT       NOT NULL,
    CONSTRAINT chk_notify_connectors_status
        CHECK (status IN ('unknown', 'connected', 'error')),
    CONSTRAINT chk_notify_connectors_test_status
        CHECK (last_test_status IS NULL OR last_test_status IN ('success', 'failed')),
    CONSTRAINT chk_notify_connectors_envelope
        CHECK (octet_length(config_nonce) = 12 AND octet_length(config_ciphertext) > 16),
    CONSTRAINT uq_notify_connectors_org_id UNIQUE (organization_id, id)
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_notify_connectors_org_name
    ON notify_connectors(organization_id, name);
CREATE UNIQUE INDEX IF NOT EXISTS uq_notify_connectors_legacy_channel
    ON notify_connectors(organization_id, legacy_channel_id)
    WHERE legacy_channel_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_notify_connectors_org_type
    ON notify_connectors(organization_id, connector_type, enabled);

CREATE TABLE IF NOT EXISTS user_notify_endpoints (
    id                  VARCHAR(64)  PRIMARY KEY,
    organization_id     VARCHAR(64)  NOT NULL,
    user_id             VARCHAR(64)  NOT NULL,
    connector_id        VARCHAR(64)  NOT NULL,
    provider_type       VARCHAR(64)  NOT NULL,
    external_identity   TEXT         NOT NULL,
    display_name        VARCHAR(255),
    metadata            JSONB        NOT NULL DEFAULT '{}'::JSONB,
    verified            BOOLEAN      NOT NULL DEFAULT FALSE,
    enabled             BOOLEAN      NOT NULL DEFAULT TRUE,
    created_at_micros   BIGINT       NOT NULL,
    updated_at_micros   BIGINT       NOT NULL,
    CONSTRAINT fk_user_notify_endpoint_connector
        FOREIGN KEY (organization_id, connector_id)
        REFERENCES notify_connectors(organization_id, id)
        ON DELETE RESTRICT,
    CONSTRAINT chk_user_notify_endpoint_identity
        CHECK (length(btrim(external_identity)) > 0)
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_user_notify_endpoint_identity
    ON user_notify_endpoints(
        organization_id,
        user_id,
        connector_id,
        external_identity
    );
CREATE INDEX IF NOT EXISTS idx_user_notify_endpoints_user
    ON user_notify_endpoints(organization_id, user_id, enabled);
CREATE INDEX IF NOT EXISTS idx_user_notify_endpoints_connector
    ON user_notify_endpoints(organization_id, connector_id);

CREATE TABLE IF NOT EXISTS user_notify_preferences (
    id                      VARCHAR(64) PRIMARY KEY,
    organization_id         VARCHAR(64) NOT NULL,
    user_id                 VARCHAR(64) NOT NULL,
    category                VARCHAR(32) NOT NULL,
    enabled                 BOOLEAN     NOT NULL DEFAULT TRUE,
    quiet_hours             JSONB,
    allow_critical_bypass   BOOLEAN     NOT NULL DEFAULT TRUE,
    created_at_micros       BIGINT      NOT NULL,
    updated_at_micros       BIGINT      NOT NULL,
    CONSTRAINT chk_user_notify_preference_category
        CHECK (category IN (
            'alert',
            'oncall',
            'escalation',
            'report',
            'security',
            'system'
        ))
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_user_notify_preference_category
    ON user_notify_preferences(organization_id, user_id, category);
CREATE INDEX IF NOT EXISTS idx_user_notify_preferences_user
    ON user_notify_preferences(organization_id, user_id);

CREATE TABLE IF NOT EXISTS user_notify_preference_steps (
    id                  VARCHAR(64) PRIMARY KEY,
    preference_id       VARCHAR(64) NOT NULL,
    endpoint_id         VARCHAR(64) NOT NULL,
    step_order          INTEGER     NOT NULL,
    created_at_micros   BIGINT      NOT NULL,
    CONSTRAINT fk_user_notify_preference_step_preference
        FOREIGN KEY (preference_id)
        REFERENCES user_notify_preferences(id)
        ON DELETE CASCADE,
    CONSTRAINT fk_user_notify_preference_step_endpoint
        FOREIGN KEY (endpoint_id)
        REFERENCES user_notify_endpoints(id)
        ON DELETE RESTRICT,
    CONSTRAINT chk_user_notify_preference_step_order CHECK (step_order >= 1)
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_user_notify_preference_step_order
    ON user_notify_preference_steps(preference_id, step_order);
CREATE UNIQUE INDEX IF NOT EXISTS uq_user_notify_preference_endpoint
    ON user_notify_preference_steps(preference_id, endpoint_id);

CREATE TABLE IF NOT EXISTS notify_deliveries (
    id                          VARCHAR(64)  PRIMARY KEY,
    organization_id             VARCHAR(64)  NOT NULL,
    event_id                    VARCHAR(255) NOT NULL,
    policy_id                   VARCHAR(64),
    recipient_user_id           VARCHAR(64),
    connector_id                VARCHAR(64),
    endpoint_id                 VARCHAR(64),
    target_type                 VARCHAR(32)  NOT NULL,
    target_value_masked         TEXT,
    stage                       VARCHAR(32)  NOT NULL,
    attempt                     INTEGER      NOT NULL DEFAULT 1,
    status                      VARCHAR(24)  NOT NULL,
    error_code                  VARCHAR(64),
    error_message               TEXT,
    latency_ms                  INTEGER,
    sent_at_micros              BIGINT,
    delivered_at_micros         BIGINT,
    acknowledged_at_micros      BIGINT,
    idempotency_key             VARCHAR(255) NOT NULL,
    created_at_micros           BIGINT       NOT NULL,
    CONSTRAINT chk_notify_delivery_stage
        CHECK (stage IN (
            'user_primary',
            'user_fallback',
            'team_fallback',
            'organization_fallback',
            'escalation'
        )),
    CONSTRAINT chk_notify_delivery_status
        CHECK (status IN (
            'pending',
            'sending',
            'success',
            'failed',
            'skipped',
            'acknowledged'
        )),
    CONSTRAINT chk_notify_delivery_attempt CHECK (attempt >= 1),
    CONSTRAINT chk_notify_delivery_latency CHECK (latency_ms IS NULL OR latency_ms >= 0)
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_notify_delivery_idempotency
    ON notify_deliveries(idempotency_key);
CREATE INDEX IF NOT EXISTS idx_notify_deliveries_org_created
    ON notify_deliveries(organization_id, created_at_micros DESC);
CREATE INDEX IF NOT EXISTS idx_notify_deliveries_event
    ON notify_deliveries(organization_id, event_id, created_at_micros);
CREATE INDEX IF NOT EXISTS idx_notify_deliveries_recipient
    ON notify_deliveries(organization_id, recipient_user_id, created_at_micros DESC);
CREATE INDEX IF NOT EXISTS idx_notify_deliveries_status
    ON notify_deliveries(organization_id, status, created_at_micros DESC);

-- ============================================================
-- Notify routing engine
-- ============================================================

CREATE UNIQUE INDEX IF NOT EXISTS uq_teams_org_id
    ON teams(org_id, id);

CREATE TABLE IF NOT EXISTS team_notify_defaults (
    id                  VARCHAR(64) PRIMARY KEY,
    organization_id     VARCHAR(64) NOT NULL,
    team_id             VARCHAR(64) NOT NULL,
    category            VARCHAR(32) NOT NULL,
    routes              JSONB       NOT NULL DEFAULT '[]'::JSONB,
    enabled             BOOLEAN     NOT NULL DEFAULT TRUE,
    created_at_micros   BIGINT      NOT NULL,
    updated_at_micros   BIGINT      NOT NULL,
    CONSTRAINT fk_team_notify_default_team
        FOREIGN KEY (organization_id, team_id)
        REFERENCES teams(org_id, id)
        ON DELETE CASCADE,
    CONSTRAINT chk_team_notify_default_category
        CHECK (category IN (
            'alert',
            'oncall',
            'escalation',
            'report',
            'security',
            'system'
        )),
    CONSTRAINT chk_team_notify_default_routes
        CHECK (jsonb_typeof(routes) = 'array'),
    CONSTRAINT uq_team_notify_default_org_id
        UNIQUE (organization_id, id)
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_team_notify_default_category
    ON team_notify_defaults(organization_id, team_id, category);
CREATE INDEX IF NOT EXISTS idx_team_notify_defaults_team
    ON team_notify_defaults(organization_id, team_id, enabled);

CREATE TABLE IF NOT EXISTS organization_notify_defaults (
    id                  VARCHAR(64) PRIMARY KEY,
    organization_id     VARCHAR(64) NOT NULL,
    category            VARCHAR(32) NOT NULL,
    routes              JSONB       NOT NULL DEFAULT '[]'::JSONB,
    enabled             BOOLEAN     NOT NULL DEFAULT TRUE,
    created_at_micros   BIGINT      NOT NULL,
    updated_at_micros   BIGINT      NOT NULL,
    CONSTRAINT chk_organization_notify_default_category
        CHECK (category IN (
            'alert',
            'oncall',
            'escalation',
            'report',
            'security',
            'system'
        )),
    CONSTRAINT chk_organization_notify_default_routes
        CHECK (jsonb_typeof(routes) = 'array'),
    CONSTRAINT uq_organization_notify_default_org_id
        UNIQUE (organization_id, id)
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_organization_notify_default_category
    ON organization_notify_defaults(organization_id, category);
CREATE INDEX IF NOT EXISTS idx_organization_notify_defaults_org
    ON organization_notify_defaults(organization_id, enabled);

CREATE TABLE IF NOT EXISTS notify_policies (
    id                      VARCHAR(64)  PRIMARY KEY,
    organization_id         VARCHAR(64)  NOT NULL,
    name                    VARCHAR(255) NOT NULL,
    event_type              VARCHAR(128) NOT NULL,
    category                VARCHAR(32)  NOT NULL,
    matchers                JSONB        NOT NULL DEFAULT '{}'::JSONB,
    recipient_resolver      VARCHAR(64)  NOT NULL,
    resolver_config         JSONB        NOT NULL DEFAULT '{}'::JSONB,
    delivery_mode           VARCHAR(32)  NOT NULL,
    template_id             VARCHAR(64),
    fallback_config         JSONB        NOT NULL DEFAULT '{}'::JSONB,
    ack_timeout_seconds     INTEGER,
    escalation_config       JSONB,
    enabled                 BOOLEAN      NOT NULL DEFAULT TRUE,
    priority                INTEGER      NOT NULL DEFAULT 100,
    created_at_micros       BIGINT       NOT NULL,
    updated_at_micros       BIGINT       NOT NULL,
    CONSTRAINT chk_notify_policy_event_type
        CHECK (length(btrim(event_type)) > 0),
    CONSTRAINT chk_notify_policy_category
        CHECK (category IN (
            'alert',
            'oncall',
            'escalation',
            'report',
            'security',
            'system'
        )),
    CONSTRAINT chk_notify_policy_delivery_mode
        CHECK (delivery_mode IN (
            'prefer_user',
            'force_connector',
            'multi_connector'
        )),
    CONSTRAINT chk_notify_policy_matchers
        CHECK (jsonb_typeof(matchers) = 'object'),
    CONSTRAINT chk_notify_policy_resolver_config
        CHECK (jsonb_typeof(resolver_config) = 'object'),
    CONSTRAINT chk_notify_policy_fallback_config
        CHECK (jsonb_typeof(fallback_config) = 'object'),
    CONSTRAINT chk_notify_policy_escalation_config
        CHECK (
            escalation_config IS NULL
            OR jsonb_typeof(escalation_config) = 'object'
        ),
    CONSTRAINT chk_notify_policy_ack_timeout
        CHECK (ack_timeout_seconds IS NULL OR ack_timeout_seconds > 0),
    CONSTRAINT chk_notify_policy_priority
        CHECK (priority BETWEEN 0 AND 10000),
    CONSTRAINT uq_notify_policies_org_id
        UNIQUE (organization_id, id)
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_notify_policies_org_name
    ON notify_policies(organization_id, name);
CREATE INDEX IF NOT EXISTS idx_notify_policies_match
    ON notify_policies(organization_id, event_type, enabled, priority, id);

-- ============================================================
-- Notify event delivery
-- ============================================================

ALTER TABLE notify_policies
    ADD COLUMN IF NOT EXISTS delivery_config JSONB NOT NULL DEFAULT '{}'::JSONB;

ALTER TABLE notify_policies
    DROP CONSTRAINT IF EXISTS chk_notify_policy_delivery_config;
ALTER TABLE notify_policies
    ADD CONSTRAINT chk_notify_policy_delivery_config
        CHECK (jsonb_typeof(delivery_config) = 'object');

ALTER TABLE notify_deliveries
    ADD COLUMN IF NOT EXISTS escalated_at_micros BIGINT;

CREATE INDEX IF NOT EXISTS idx_notify_deliveries_ack_timeout
    ON notify_deliveries(
        organization_id,
        delivered_at_micros,
        escalated_at_micros
    )
    WHERE status = 'success'
      AND acknowledged_at_micros IS NULL;

CREATE TABLE IF NOT EXISTS notify_events (
    id                      VARCHAR(255) NOT NULL,
    organization_id         VARCHAR(64)  NOT NULL,
    event_type              VARCHAR(128) NOT NULL,
    occurred_at_micros      BIGINT       NOT NULL,
    attributes              JSONB        NOT NULL DEFAULT '{}'::JSONB,
    message                 JSONB        NOT NULL,
    status                  VARCHAR(24)  NOT NULL DEFAULT 'pending',
    attempt                 INTEGER      NOT NULL DEFAULT 0,
    next_attempt_at_micros  BIGINT       NOT NULL,
    claimed_at_micros       BIGINT,
    last_error              TEXT,
    created_at_micros       BIGINT       NOT NULL,
    updated_at_micros       BIGINT       NOT NULL,
    CONSTRAINT chk_notify_event_status
        CHECK (status IN ('pending', 'processing', 'completed', 'failed')),
    CONSTRAINT chk_notify_event_attempt CHECK (attempt >= 0),
    CONSTRAINT chk_notify_event_attributes
        CHECK (jsonb_typeof(attributes) = 'object'),
    CONSTRAINT chk_notify_event_message
        CHECK (jsonb_typeof(message) = 'object'),
    CONSTRAINT pk_notify_events PRIMARY KEY (organization_id, id)
);

CREATE INDEX IF NOT EXISTS idx_notify_events_pending
    ON notify_events(
        organization_id,
        status,
        next_attempt_at_micros,
        created_at_micros
    );

-- ============================================================
-- Canonical Notify storage
-- ============================================================

-- Alert delivery uses notify_connectors, notify_policies, notify_events and
-- notify_deliveries as its canonical storage.

DROP TABLE IF EXISTS deliveries;
DROP TABLE IF EXISTS notify_channels;
DROP TABLE IF EXISTS alert_subscriptions;

ALTER TABLE notify_connectors
    DROP COLUMN IF EXISTS legacy_channel_id;

ALTER TABLE alert_rules
    DROP COLUMN IF EXISTS template_id,
    DROP COLUMN IF EXISTS body_template;

ALTER TABLE incidents
    DROP COLUMN IF EXISTS body_template;

ALTER TABLE IF EXISTS alert_templates
    RENAME TO notify_templates;

ALTER INDEX IF EXISTS uq_alert_template_org_name
    RENAME TO uq_notify_template_org_name;

-- ============================================================
-- Final Notify schema
-- ============================================================

-- Finalize the Notify schema defined in the preceding sections.

ALTER TABLE notify_deliveries
    DROP CONSTRAINT IF EXISTS chk_notify_delivery_stage;
ALTER TABLE notify_deliveries
    ADD CONSTRAINT chk_notify_delivery_stage
        CHECK (stage IN (
            'user_primary',
            'user_fallback',
            'team_fallback',
            'organization_fallback',
            'escalation',
            'test'
        ));

ALTER TABLE notify_policies
    DROP CONSTRAINT IF EXISTS fk_notify_policy_template;
ALTER TABLE notify_policies
    ADD CONSTRAINT fk_notify_policy_template
        FOREIGN KEY (template_id)
        REFERENCES notify_templates(id)
        ON DELETE RESTRICT;

-- ============================================================
-- Notify template categories
-- ============================================================

ALTER TABLE notify_templates
    ADD COLUMN IF NOT EXISTS category VARCHAR(32) NOT NULL DEFAULT 'alert';

ALTER TABLE notify_templates
    DROP CONSTRAINT IF EXISTS chk_notify_template_category;
ALTER TABLE notify_templates
    ADD CONSTRAINT chk_notify_template_category
        CHECK (category IN (
            'alert',
            'oncall',
            'escalation',
            'report',
            'security',
            'system'
        ));

CREATE INDEX IF NOT EXISTS idx_notify_templates_org_category
    ON notify_templates(org_id, category, name);

-- ============================================================
-- Application Performance Monitoring
-- ============================================================

-- Application Performance Monitoring: bounded owner snapshots and hourly rollups.

CREATE TABLE IF NOT EXISTS apm_services (
    org_id                 VARCHAR(64)  NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    service_namespace      VARCHAR(128) NOT NULL,
    service_name           VARCHAR(255) NOT NULL,
    environment            VARCHAR(128) NOT NULL,
    first_seen_at_micros   BIGINT       NOT NULL,
    last_seen_at_micros    BIGINT       NOT NULL,
    runtime_language       VARCHAR(64),
    telemetry_sdk_name     VARCHAR(128),
    telemetry_sdk_version  VARCHAR(64),
    recent_instance_count  INTEGER      NOT NULL DEFAULT 0 CHECK (recent_instance_count >= 0),
    PRIMARY KEY (org_id, service_namespace, service_name, environment),
    CHECK (service_namespace <> '' AND service_name <> '' AND environment <> ''),
    CHECK (last_seen_at_micros >= first_seen_at_micros)
);
CREATE INDEX IF NOT EXISTS idx_apm_services_org_last_seen
    ON apm_services (org_id, last_seen_at_micros DESC);

CREATE TABLE IF NOT EXISTS apm_service_versions (
    org_id                VARCHAR(64)  NOT NULL,
    service_namespace     VARCHAR(128) NOT NULL,
    service_name          VARCHAR(255) NOT NULL,
    environment           VARCHAR(128) NOT NULL,
    version               VARCHAR(128) NOT NULL,
    first_seen_at_micros  BIGINT       NOT NULL,
    last_seen_at_micros   BIGINT       NOT NULL,
    observation_count     BIGINT       NOT NULL DEFAULT 0 CHECK (observation_count >= 0),
    PRIMARY KEY (org_id, service_namespace, service_name, environment, version),
    FOREIGN KEY (org_id, service_namespace, service_name, environment)
        REFERENCES apm_services (org_id, service_namespace, service_name, environment)
        ON DELETE CASCADE,
    CHECK (version <> '' AND last_seen_at_micros >= first_seen_at_micros)
);
CREATE INDEX IF NOT EXISTS idx_apm_service_versions_org_seen
    ON apm_service_versions (org_id, last_seen_at_micros DESC);

CREATE TABLE IF NOT EXISTS apm_error_groups (
    org_id                  VARCHAR(64)  NOT NULL,
    fingerprint             VARCHAR(128) NOT NULL,
    service_namespace       VARCHAR(128) NOT NULL,
    service_name            VARCHAR(255) NOT NULL,
    environment             VARCHAR(128) NOT NULL,
    error_identity          JSONB        NOT NULL CHECK (jsonb_typeof(error_identity) = 'object'),
    first_seen_at_micros    BIGINT       NOT NULL,
    last_seen_at_micros     BIGINT       NOT NULL,
    occurrence_count        BIGINT       NOT NULL DEFAULT 0 CHECK (occurrence_count >= 0),
    representative_message TEXT,
    representative_stack   JSONB        NOT NULL DEFAULT '[]'::jsonb
        CHECK (jsonb_typeof(representative_stack) = 'array'),
    PRIMARY KEY (org_id, fingerprint),
    FOREIGN KEY (org_id, service_namespace, service_name, environment)
        REFERENCES apm_services (org_id, service_namespace, service_name, environment)
        ON DELETE CASCADE,
    CHECK (fingerprint <> '' AND last_seen_at_micros >= first_seen_at_micros)
);
CREATE INDEX IF NOT EXISTS idx_apm_error_groups_org_last_seen
    ON apm_error_groups (org_id, last_seen_at_micros DESC);
CREATE INDEX IF NOT EXISTS idx_apm_error_groups_org_service
    ON apm_error_groups (
        org_id, service_namespace, service_name, environment, last_seen_at_micros DESC
    );

CREATE TABLE IF NOT EXISTS apm_error_samples (
    org_id                  VARCHAR(64)  NOT NULL,
    fingerprint             VARCHAR(128) NOT NULL,
    sample_slot             SMALLINT     NOT NULL CHECK (sample_slot >= 0),
    event_time_micros       BIGINT       NOT NULL,
    trace_id                VARCHAR(64)  NOT NULL,
    span_id                 VARCHAR(32)  NOT NULL,
    trace_available         BOOLEAN      NOT NULL DEFAULT FALSE,
    representative_message TEXT,
    representative_stack   JSONB        NOT NULL DEFAULT '[]'::jsonb
        CHECK (jsonb_typeof(representative_stack) = 'array'),
    PRIMARY KEY (org_id, fingerprint, sample_slot),
    FOREIGN KEY (org_id, fingerprint)
        REFERENCES apm_error_groups (org_id, fingerprint) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_apm_error_samples_org_time
    ON apm_error_samples (org_id, event_time_micros DESC);

DO $apm_buckets$
DECLARE
    bucket_kind TEXT;
    minute_table TEXT;
    hourly_table TEXT;
    default_partition TEXT;
BEGIN
    FOREACH bucket_kind IN ARRAY ARRAY['service', 'transaction', 'dependency', 'error']
    LOOP
        minute_table := format('apm_%s_buckets', bucket_kind);
        hourly_table := format('apm_%s_buckets_hourly', bucket_kind);
        default_partition := minute_table || '_default';

        EXECUTE format(
            'CREATE TABLE IF NOT EXISTS %I (
                org_id VARCHAR(64) NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
                service_namespace VARCHAR(128) NOT NULL,
                service_name VARCHAR(255) NOT NULL,
                environment VARCHAR(128) NOT NULL,
                version VARCHAR(128) NOT NULL DEFAULT '''',
                bucket_at_micros BIGINT NOT NULL,
                owner_id VARCHAR(128) NOT NULL,
                snapshot_seq BIGINT NOT NULL CHECK (snapshot_seq >= 0),
                persistence_schema_version SMALLINT NOT NULL CHECK (persistence_schema_version > 0),
                histogram_schema_version SMALLINT NOT NULL CHECK (histogram_schema_version > 0),
                dimension_key BYTEA NOT NULL,
                dimension JSONB NOT NULL CHECK (jsonb_typeof(dimension) = ''object''),
                measurements JSONB NOT NULL CHECK (jsonb_typeof(measurements) = ''object''),
                updated_at_micros BIGINT NOT NULL,
                PRIMARY KEY (
                    org_id, bucket_at_micros, dimension_key, owner_id,
                    histogram_schema_version
                ),
                CHECK (
                    service_namespace <> '''' AND service_name <> '''' AND
                    environment <> '''' AND owner_id <> '''' AND
                    octet_length(dimension_key) = 32
                )
            ) PARTITION BY RANGE (bucket_at_micros)',
            minute_table
        );
        EXECUTE format(
            'CREATE TABLE IF NOT EXISTS %I PARTITION OF %I DEFAULT',
            default_partition,
            minute_table
        );
        EXECUTE format(
            'CREATE INDEX IF NOT EXISTS %I ON %I (org_id, bucket_at_micros DESC)',
            'idx_' || minute_table || '_org_time',
            minute_table
        );
        EXECUTE format(
            'CREATE INDEX IF NOT EXISTS %I ON %I (
                org_id, service_namespace, service_name, environment, bucket_at_micros DESC
            )',
            'idx_' || minute_table || '_org_service_time',
            minute_table
        );
        EXECUTE format(
            'CREATE INDEX IF NOT EXISTS %I ON %I USING BRIN (bucket_at_micros)',
            'idx_' || minute_table || '_retention_brin',
            minute_table
        );

        EXECUTE format(
            'CREATE TABLE IF NOT EXISTS %I (
                org_id VARCHAR(64) NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
                service_namespace VARCHAR(128) NOT NULL,
                service_name VARCHAR(255) NOT NULL,
                environment VARCHAR(128) NOT NULL,
                version VARCHAR(128) NOT NULL DEFAULT '''',
                bucket_at_micros BIGINT NOT NULL,
                persistence_schema_version SMALLINT NOT NULL CHECK (persistence_schema_version > 0),
                histogram_schema_version SMALLINT NOT NULL CHECK (histogram_schema_version > 0),
                dimension_key BYTEA NOT NULL,
                dimension JSONB NOT NULL CHECK (jsonb_typeof(dimension) = ''object''),
                measurements JSONB NOT NULL CHECK (jsonb_typeof(measurements) = ''object''),
                source_minute_count SMALLINT NOT NULL CHECK (
                    source_minute_count BETWEEN 1 AND 60
                ),
                completed_at_micros BIGINT NOT NULL,
                PRIMARY KEY (
                    org_id, bucket_at_micros, dimension_key, histogram_schema_version
                ),
                CHECK (
                    service_namespace <> '''' AND service_name <> '''' AND
                    environment <> '''' AND octet_length(dimension_key) = 32
                )
            ) PARTITION BY RANGE (bucket_at_micros)',
            hourly_table
        );
        EXECUTE format(
            'CREATE TABLE IF NOT EXISTS %I PARTITION OF %I DEFAULT',
            hourly_table || '_default',
            hourly_table
        );
        EXECUTE format(
            'CREATE INDEX IF NOT EXISTS %I ON %I (org_id, bucket_at_micros DESC)',
            'idx_' || hourly_table || '_org_time',
            hourly_table
        );
        EXECUTE format(
            'CREATE INDEX IF NOT EXISTS %I ON %I (
                org_id, service_namespace, service_name, environment, bucket_at_micros DESC
            )',
            'idx_' || hourly_table || '_org_service_time',
            hourly_table
        );
        EXECUTE format(
            'CREATE INDEX IF NOT EXISTS %I ON %I USING BRIN (bucket_at_micros)',
            'idx_' || hourly_table || '_retention_brin',
            hourly_table
        );
    END LOOP;
END
$apm_buckets$;

CREATE TABLE IF NOT EXISTS apm_projection_gaps (
    id                    VARCHAR(64) NOT NULL,
    org_id                VARCHAR(64) NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    range_start_micros    BIGINT      NOT NULL,
    range_end_micros      BIGINT      NOT NULL,
    reason                VARCHAR(64) NOT NULL,
    dropped_facts         BIGINT      NOT NULL CHECK (dropped_facts >= 0),
    recorded_at_micros    BIGINT      NOT NULL,
    PRIMARY KEY (org_id, id),
    CHECK (range_end_micros >= range_start_micros)
);
CREATE INDEX IF NOT EXISTS idx_apm_projection_gaps_org_range
    ON apm_projection_gaps (org_id, range_start_micros, range_end_micros);

CREATE TABLE IF NOT EXISTS apm_projection_state (
    org_id                         VARCHAR(64) PRIMARY KEY
        REFERENCES organizations(id) ON DELETE CASCADE,
    projection_started_at_micros   BIGINT NOT NULL,
    last_complete_bucket_at_micros BIGINT,
    last_rollup_bucket_at_micros   BIGINT
);

CREATE TABLE IF NOT EXISTS apm_rollup_state (
    org_id                       VARCHAR(64) NOT NULL
        REFERENCES organizations(id) ON DELETE CASCADE,
    bucket_kind                  VARCHAR(32) NOT NULL,
    histogram_schema_version     SMALLINT    NOT NULL CHECK (histogram_schema_version > 0),
    completed_through_micros     BIGINT      NOT NULL,
    updated_at_micros            BIGINT      NOT NULL,
    PRIMARY KEY (org_id, bucket_kind, histogram_schema_version),
    CHECK (bucket_kind IN ('service', 'transaction', 'dependency', 'error'))
);
-- ============================================================
-- Status page publication schema
-- ============================================================

-- SPDX-License-Identifier: Apache-2.0
-- Copyright (c) 2026 MoleSignal Authors

-- Customer-facing status pages are an organization-scoped publication layer
-- over internal incidents. The composite key keeps source-incident references
-- tenant-safe.
ALTER TABLE incidents
    ADD CONSTRAINT uq_incidents_org_id_id
    UNIQUE (org_id, id);

CREATE TABLE status_pages (
    id                  VARCHAR(64)  PRIMARY KEY,
    org_id              VARCHAR(64)  NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name                VARCHAR(120) NOT NULL,
    slug                VARCHAR(64)  NOT NULL UNIQUE,
    logo_url            TEXT,
    brand_color         VARCHAR(7)   NOT NULL DEFAULT '#4F46E5',
    custom_domain       VARCHAR(253),
    timezone            VARCHAR(64)  NOT NULL DEFAULT 'UTC',
    language            VARCHAR(16)  NOT NULL DEFAULT 'en-us',
    visibility          VARCHAR(16)  NOT NULL DEFAULT 'public',
    created_at_micros   BIGINT       NOT NULL,
    updated_at_micros   BIGINT       NOT NULL,
    CONSTRAINT uq_status_pages_org_id_id UNIQUE (org_id, id),
    CONSTRAINT chk_status_pages_slug
        CHECK (slug ~ '^[a-z0-9]+(-[a-z0-9]+)*$' AND length(slug) BETWEEN 3 AND 64),
    CONSTRAINT chk_status_pages_brand_color
        CHECK (brand_color ~ '^#[0-9A-F]{6}$'),
    CONSTRAINT chk_status_pages_language
        CHECK (language IN ('en-us', 'zh-cn')),
    CONSTRAINT chk_status_pages_visibility
        CHECK (visibility IN ('public', 'private'))
);
CREATE INDEX idx_status_pages_org
    ON status_pages(org_id, updated_at_micros DESC);
CREATE UNIQUE INDEX uq_status_pages_custom_domain
    ON status_pages(lower(custom_domain)) WHERE custom_domain IS NOT NULL;

CREATE TABLE status_page_components (
    id                  VARCHAR(64)  PRIMARY KEY,
    org_id              VARCHAR(64)  NOT NULL,
    status_page_id      VARCHAR(64)  NOT NULL,
    name                VARCHAR(120) NOT NULL,
    description         VARCHAR(500) NOT NULL DEFAULT '',
    status              VARCHAR(32)  NOT NULL DEFAULT 'operational',
    position            INTEGER      NOT NULL DEFAULT 0,
    created_at_micros   BIGINT       NOT NULL,
    updated_at_micros   BIGINT       NOT NULL,
    CONSTRAINT uq_status_page_components_scope UNIQUE (org_id, status_page_id, id),
    CONSTRAINT fk_status_page_components_page
        FOREIGN KEY (org_id, status_page_id)
        REFERENCES status_pages(org_id, id) ON DELETE CASCADE,
    CONSTRAINT chk_status_page_component_status
        CHECK (status IN (
            'operational', 'degraded_performance', 'partial_outage',
            'major_outage', 'maintenance'
        ))
);
CREATE UNIQUE INDEX uq_status_page_components_name
    ON status_page_components(org_id, status_page_id, lower(name));
CREATE INDEX idx_status_page_components_order
    ON status_page_components(org_id, status_page_id, position, name);

CREATE TABLE status_page_incidents (
    id                      VARCHAR(64)  PRIMARY KEY,
    org_id                  VARCHAR(64)  NOT NULL,
    status_page_id          VARCHAR(64)  NOT NULL,
    source_incident_id      VARCHAR(64),
    kind                    VARCHAR(16)  NOT NULL,
    title                   VARCHAR(200) NOT NULL,
    impact                  VARCHAR(16)  NOT NULL,
    status                  VARCHAR(32)  NOT NULL,
    started_at_micros       BIGINT       NOT NULL,
    resolved_at_micros      BIGINT,
    created_at_micros       BIGINT       NOT NULL,
    updated_at_micros       BIGINT       NOT NULL,
    CONSTRAINT uq_status_page_incidents_scope UNIQUE (org_id, status_page_id, id),
    CONSTRAINT fk_status_page_incidents_page
        FOREIGN KEY (org_id, status_page_id)
        REFERENCES status_pages(org_id, id) ON DELETE CASCADE,
    CONSTRAINT fk_status_page_incidents_source
        FOREIGN KEY (org_id, source_incident_id)
        REFERENCES incidents(org_id, id) ON DELETE RESTRICT,
    CONSTRAINT chk_status_page_incident_kind
        CHECK (kind IN ('incident', 'maintenance')),
    CONSTRAINT chk_status_page_incident_impact
        CHECK (impact IN ('minor', 'major', 'critical', 'maintenance')),
    CONSTRAINT chk_status_page_incident_status
        CHECK (status IN ('investigating', 'identified', 'in_progress', 'monitoring', 'resolved')),
    CONSTRAINT chk_status_page_incident_resolution
        CHECK ((status = 'resolved') = (resolved_at_micros IS NOT NULL)),
    CONSTRAINT chk_status_page_incident_kind_impact
        CHECK (
            (kind = 'incident' AND impact <> 'maintenance' AND source_incident_id IS NOT NULL)
            OR (kind = 'maintenance' AND impact = 'maintenance' AND source_incident_id IS NULL)
        )
);
CREATE UNIQUE INDEX uq_status_page_incident_source
    ON status_page_incidents(org_id, status_page_id, source_incident_id)
    WHERE source_incident_id IS NOT NULL;
CREATE INDEX idx_status_page_incidents_timeline
    ON status_page_incidents(org_id, status_page_id, started_at_micros DESC);
CREATE INDEX idx_status_page_incidents_active
    ON status_page_incidents(org_id, status_page_id, status, started_at_micros)
    WHERE status <> 'resolved';
CREATE INDEX idx_status_page_incidents_resolved
    ON status_page_incidents(org_id, status_page_id, resolved_at_micros DESC)
    WHERE status = 'resolved';

CREATE TABLE status_page_incident_components (
    org_id              VARCHAR(64) NOT NULL,
    status_page_id      VARCHAR(64) NOT NULL,
    incident_id         VARCHAR(64) NOT NULL,
    component_id        VARCHAR(64) NOT NULL,
    PRIMARY KEY (org_id, status_page_id, incident_id, component_id),
    CONSTRAINT fk_status_page_incident_components_incident
        FOREIGN KEY (org_id, status_page_id, incident_id)
        REFERENCES status_page_incidents(org_id, status_page_id, id) ON DELETE CASCADE,
    CONSTRAINT fk_status_page_incident_components_component
        FOREIGN KEY (org_id, status_page_id, component_id)
        REFERENCES status_page_components(org_id, status_page_id, id) ON DELETE RESTRICT
);
CREATE INDEX idx_status_page_incident_components_component
    ON status_page_incident_components(org_id, status_page_id, component_id);

CREATE TABLE status_page_incident_updates (
    id                  VARCHAR(64)   PRIMARY KEY,
    org_id              VARCHAR(64)   NOT NULL,
    status_page_id      VARCHAR(64)   NOT NULL,
    incident_id         VARCHAR(64)   NOT NULL,
    status              VARCHAR(32)   NOT NULL,
    message             VARCHAR(4000) NOT NULL,
    created_at_micros   BIGINT        NOT NULL,
    CONSTRAINT fk_status_page_incident_updates_incident
        FOREIGN KEY (org_id, status_page_id, incident_id)
        REFERENCES status_page_incidents(org_id, status_page_id, id) ON DELETE CASCADE,
    CONSTRAINT chk_status_page_incident_update_status
        CHECK (status IN ('investigating', 'identified', 'in_progress', 'monitoring', 'resolved')),
    CONSTRAINT chk_status_page_incident_update_message
        CHECK (length(btrim(message)) BETWEEN 1 AND 4000)
);
CREATE INDEX idx_status_page_incident_updates_timeline
    ON status_page_incident_updates(org_id, status_page_id, incident_id, created_at_micros DESC);

-- ============================================================
-- Status page languages
-- ============================================================

-- SPDX-License-Identifier: Apache-2.0
-- Copyright (c) 2026 MoleSignal Authors

ALTER TABLE status_pages
    ADD COLUMN languages TEXT[] NOT NULL DEFAULT ARRAY['en-us']::TEXT[];

ALTER TABLE status_pages
    ADD CONSTRAINT chk_status_pages_languages
    CHECK (
        cardinality(languages) BETWEEN 1 AND 2
        AND array_lower(languages, 1) = 1
        AND array_position(languages, NULL) IS NULL
        AND languages <@ ARRAY['en-us', 'zh-cn']::TEXT[]
        AND language = languages[1]
        AND (cardinality(languages) = 1 OR languages[1] <> languages[2])
    );

-- ============================================================
-- Status page public history window
-- ============================================================

-- SPDX-License-Identifier: Apache-2.0
-- Copyright (c) 2026 MoleSignal Authors

-- The configured window controls customer-visible resolved incident history.
-- Uptime calculations may still read at least 90 days of incident data.
ALTER TABLE status_pages
    ADD COLUMN history_days INTEGER NOT NULL DEFAULT 90;

ALTER TABLE status_pages
    ADD CONSTRAINT chk_status_pages_history_days
    CHECK (history_days BETWEEN 1 AND 365);

-- ============================================================
-- Status page component history
-- ============================================================

-- SPDX-License-Identifier: Apache-2.0
-- Copyright (c) 2026 MoleSignal Authors

-- Persist the component's manually managed status timeline. Published
-- incidents remain a separate overlay, so an incident can temporarily worsen
-- a component without destroying the operator-managed baseline.
CREATE TABLE status_page_component_status_events (
    id                  VARCHAR(64) NOT NULL PRIMARY KEY,
    org_id              VARCHAR(64) NOT NULL,
    status_page_id      VARCHAR(64) NOT NULL,
    component_id        VARCHAR(64) NOT NULL,
    status              VARCHAR(32) NOT NULL,
    started_at_micros   BIGINT      NOT NULL,
    ended_at_micros     BIGINT,
    created_at_micros   BIGINT      NOT NULL,
    CONSTRAINT fk_status_page_component_status_events_component
        FOREIGN KEY (org_id, status_page_id, component_id)
        REFERENCES status_page_components(org_id, status_page_id, id) ON DELETE CASCADE,
    CONSTRAINT chk_status_page_component_status_event_status
        CHECK (status IN (
            'operational', 'degraded_performance', 'partial_outage',
            'major_outage', 'maintenance'
        )),
    CONSTRAINT chk_status_page_component_status_event_range
        CHECK (ended_at_micros IS NULL OR ended_at_micros >= started_at_micros)
);

CREATE UNIQUE INDEX uq_status_page_component_status_events_open
    ON status_page_component_status_events(org_id, status_page_id, component_id)
    WHERE ended_at_micros IS NULL;
CREATE INDEX idx_status_page_component_status_events_timeline
    ON status_page_component_status_events(
        org_id, status_page_id, component_id, started_at_micros DESC
    );

-- ============================================================
-- Status page subscriptions and delivery
-- ============================================================

-- SPDX-License-Identifier: Apache-2.0
-- Copyright (c) 2026 MoleSignal Authors

CREATE TABLE status_page_subscribers (
    id                          VARCHAR(64)  PRIMARY KEY,
    org_id                      VARCHAR(64)  NOT NULL,
    status_page_id              VARCHAR(64)  NOT NULL,
    channel                     VARCHAR(16)  NOT NULL,
    target_ciphertext           BYTEA        NOT NULL,
    target_nonce                BYTEA        NOT NULL,
    target_hash                 CHAR(64)     NOT NULL,
    status                      VARCHAR(16)  NOT NULL,
    token_hash                  CHAR(64)     NOT NULL UNIQUE,
    confirmation_sent_at_micros BIGINT,
    confirmed_at_micros         BIGINT,
    unsubscribed_at_micros      BIGINT,
    created_at_micros           BIGINT       NOT NULL,
    updated_at_micros           BIGINT       NOT NULL,
    CONSTRAINT uq_status_page_subscribers_scope UNIQUE (org_id, status_page_id, id),
    CONSTRAINT fk_status_page_subscribers_page
        FOREIGN KEY (org_id, status_page_id)
        REFERENCES status_pages(org_id, id) ON DELETE CASCADE,
    CONSTRAINT chk_status_page_subscriber_channel
        CHECK (channel IN ('email', 'webhook')),
    CONSTRAINT chk_status_page_subscriber_status
        CHECK (status IN ('pending', 'active', 'unsubscribed')),
    CONSTRAINT chk_status_page_subscriber_target_cipher
        CHECK (
            octet_length(target_nonce) = 12
            AND octet_length(target_ciphertext) BETWEEN 19 AND 8208
        )
);
CREATE UNIQUE INDEX uq_status_page_subscribers_target
    ON status_page_subscribers(org_id, status_page_id, channel, target_hash);
CREATE INDEX idx_status_page_subscribers_management
    ON status_page_subscribers(org_id, status_page_id, status, updated_at_micros DESC);

CREATE TABLE status_page_notification_deliveries (
    id                      VARCHAR(64)  PRIMARY KEY,
    org_id                  VARCHAR(64)  NOT NULL,
    status_page_id          VARCHAR(64)  NOT NULL,
    subscriber_id           VARCHAR(64)  NOT NULL,
    event_key               VARCHAR(160) NOT NULL,
    payload                 JSONB        NOT NULL,
    status                  VARCHAR(16)  NOT NULL DEFAULT 'pending',
    attempts                INTEGER      NOT NULL DEFAULT 0,
    next_attempt_at_micros  BIGINT       NOT NULL,
    claimed_at_micros       BIGINT,
    delivered_at_micros     BIGINT,
    last_error              VARCHAR(500),
    created_at_micros       BIGINT       NOT NULL,
    updated_at_micros       BIGINT       NOT NULL,
    CONSTRAINT fk_status_page_notification_delivery_subscriber
        FOREIGN KEY (org_id, status_page_id, subscriber_id)
        REFERENCES status_page_subscribers(org_id, status_page_id, id) ON DELETE CASCADE,
    CONSTRAINT uq_status_page_notification_delivery_event
        UNIQUE (subscriber_id, event_key),
    CONSTRAINT chk_status_page_notification_delivery_status
        CHECK (status IN ('pending', 'processing', 'delivered', 'failed')),
    CONSTRAINT chk_status_page_notification_delivery_attempts
        CHECK (attempts BETWEEN 0 AND 20)
);
CREATE INDEX idx_status_page_notification_deliveries_pending
    ON status_page_notification_deliveries(next_attempt_at_micros, created_at_micros)
    WHERE status IN ('pending', 'processing');
CREATE INDEX idx_status_page_notification_deliveries_audit
    ON status_page_notification_deliveries(
        org_id, status_page_id, subscriber_id, created_at_micros DESC
    );

CREATE FUNCTION enqueue_status_page_component_notification()
RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO status_page_notification_deliveries (
        id, org_id, status_page_id, subscriber_id, event_key, payload,
        next_attempt_at_micros, created_at_micros, updated_at_micros
    )
    SELECT
        md5(subscriber.id || ':component_status:' || NEW.id),
        NEW.org_id,
        NEW.status_page_id,
        subscriber.id,
        'component_status:' || NEW.id,
        jsonb_build_object(
            'type', 'component_status_changed',
            'page_name', page.name,
            'page_slug', page.slug,
            'component_id', component.id,
            'component_name', component.name,
            'status', NEW.status,
            'occurred_at', NEW.started_at_micros
        ),
        NEW.started_at_micros,
        NEW.created_at_micros,
        NEW.created_at_micros
    FROM status_page_subscribers subscriber
    JOIN status_pages page
      ON page.org_id = NEW.org_id AND page.id = NEW.status_page_id
    JOIN status_page_components component
      ON component.org_id = NEW.org_id
     AND component.status_page_id = NEW.status_page_id
     AND component.id = NEW.component_id
    WHERE subscriber.org_id = NEW.org_id
      AND subscriber.status_page_id = NEW.status_page_id
      AND subscriber.status = 'active'
      AND page.visibility = 'public'
    ON CONFLICT (subscriber_id, event_key) DO NOTHING;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_status_page_component_notification
AFTER INSERT ON status_page_component_status_events
FOR EACH ROW EXECUTE FUNCTION enqueue_status_page_component_notification();

CREATE FUNCTION enqueue_status_page_incident_notification()
RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO status_page_notification_deliveries (
        id, org_id, status_page_id, subscriber_id, event_key, payload,
        next_attempt_at_micros, created_at_micros, updated_at_micros
    )
    SELECT
        md5(subscriber.id || ':incident_update:' || NEW.id),
        NEW.org_id,
        NEW.status_page_id,
        subscriber.id,
        'incident_update:' || NEW.id,
        jsonb_build_object(
            'type', 'incident_update',
            'page_name', page.name,
            'page_slug', page.slug,
            'incident_id', incident.id,
            'incident_kind', incident.kind,
            'title', incident.title,
            'impact', incident.impact,
            'status', NEW.status,
            'message', NEW.message,
            'occurred_at', NEW.created_at_micros,
            'component_names', COALESCE((
                SELECT jsonb_agg(component.name ORDER BY component.position, component.name)
                FROM status_page_incident_components link
                JOIN status_page_components component
                  ON component.org_id = link.org_id
                 AND component.status_page_id = link.status_page_id
                 AND component.id = link.component_id
                WHERE link.org_id = NEW.org_id
                  AND link.status_page_id = NEW.status_page_id
                  AND link.incident_id = NEW.incident_id
            ), '[]'::JSONB)
        ),
        NEW.created_at_micros,
        NEW.created_at_micros,
        NEW.created_at_micros
    FROM status_page_subscribers subscriber
    JOIN status_pages page
      ON page.org_id = NEW.org_id AND page.id = NEW.status_page_id
    JOIN status_page_incidents incident
      ON incident.org_id = NEW.org_id
     AND incident.status_page_id = NEW.status_page_id
     AND incident.id = NEW.incident_id
    WHERE subscriber.org_id = NEW.org_id
      AND subscriber.status_page_id = NEW.status_page_id
      AND subscriber.status = 'active'
      AND page.visibility = 'public'
    ON CONFLICT (subscriber_id, event_key) DO NOTHING;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_status_page_incident_notification
AFTER INSERT ON status_page_incident_updates
FOR EACH ROW EXECUTE FUNCTION enqueue_status_page_incident_notification();

-- ============================================================
-- Status page management, access, domains, and automation
-- ============================================================

-- SPDX-License-Identifier: Apache-2.0
-- Copyright (c) 2026 MoleSignal Authors

INSERT INTO iam_permissions (
    permission_key, scope, domain, label_key, description_key, feature, catalog_version
)
VALUES
    ('status_pages.read', 'organization', 'status_pages',
     'permissions.status_pages_read', 'permissions_hint.status_pages_read', NULL, 8),
    ('status_pages.manage', 'organization', 'status_pages',
     'permissions.status_pages_manage', 'permissions_hint.status_pages_manage', NULL, 8)
ON CONFLICT (permission_key) DO UPDATE
SET scope = EXCLUDED.scope,
    domain = EXCLUDED.domain,
    label_key = EXCLUDED.label_key,
    description_key = EXCLUDED.description_key,
    feature = EXCLUDED.feature,
    catalog_version = EXCLUDED.catalog_version;

UPDATE iam_permission_catalog_versions
SET version = GREATEST(version, 8),
    updated_at_micros = (EXTRACT(EPOCH FROM clock_timestamp()) * 1000000)::BIGINT
WHERE catalog_key = 'permissions';

INSERT INTO iam_builtin_role_permissions (role_key, permission_key)
VALUES
    ('owner', 'status_pages.read'),
    ('owner', 'status_pages.manage'),
    ('admin', 'status_pages.read'),
    ('admin', 'status_pages.manage')
ON CONFLICT DO NOTHING;

INSERT INTO iam_role_permissions (role_id, permission_key)
SELECT role.id, permission.permission_key
FROM iam_roles role
CROSS JOIN (VALUES
    ('owner', 'status_pages.read'),
    ('owner', 'status_pages.manage'),
    ('admin', 'status_pages.read'),
    ('admin', 'status_pages.manage')
) AS permission(role_key, permission_key)
WHERE role.builtin AND role.role_key = permission.role_key
ON CONFLICT DO NOTHING;

WITH requested(bundle_key, permission_key, offset_position) AS (
    VALUES
        ('readonly_observer', 'status_pages.read', 1),
        ('alert_administrator', 'status_pages.read', 1),
        ('alert_administrator', 'status_pages.manage', 2),
        ('organization_administrator', 'status_pages.read', 1),
        ('organization_administrator', 'status_pages.manage', 2)
), base AS (
    SELECT bundle_key, COALESCE(MAX(position), -1) AS max_position
    FROM iam_permission_bundle_items
    GROUP BY bundle_key
)
INSERT INTO iam_permission_bundle_items (bundle_key, permission_key, position)
SELECT requested.bundle_key,
       requested.permission_key,
       base.max_position + requested.offset_position
FROM requested
JOIN base USING (bundle_key)
ON CONFLICT (bundle_key, permission_key) DO NOTHING;

-- Page lifecycle is recoverable for 30 days. Public history configuration is
-- intentionally independent from notification-delivery retention.
ALTER TABLE status_pages
    ADD COLUMN lifecycle VARCHAR(16) NOT NULL DEFAULT 'active',
    ADD COLUMN archived_at_micros BIGINT,
    ADD COLUMN purge_after_micros BIGINT,
    ADD COLUMN delivery_retention_days INTEGER NOT NULL DEFAULT 90,
    ADD COLUMN private_session_days INTEGER NOT NULL DEFAULT 7;
ALTER TABLE status_pages
    ADD CONSTRAINT chk_status_pages_lifecycle
        CHECK (lifecycle IN ('active', 'archived')),
    ADD CONSTRAINT chk_status_pages_archive_window
        CHECK (
            (lifecycle = 'active' AND archived_at_micros IS NULL AND purge_after_micros IS NULL)
            OR
            (lifecycle = 'archived' AND archived_at_micros IS NOT NULL
             AND purge_after_micros IS NOT NULL
             AND purge_after_micros >= archived_at_micros)
        ),
    ADD CONSTRAINT chk_status_pages_delivery_retention
        CHECK (delivery_retention_days IN (30, 60, 90, 180, 365)),
    ADD CONSTRAINT chk_status_pages_private_session_days
        CHECK (private_session_days IN (1, 7, 30));
CREATE INDEX idx_status_pages_lifecycle
    ON status_pages(org_id, lifecycle, updated_at_micros DESC);
CREATE INDEX idx_status_pages_purge_due
    ON status_pages(purge_after_micros)
    WHERE lifecycle = 'archived';

-- Visibility determines public inclusion; lifecycle determines whether the
-- component can still be edited or selected for new events.
ALTER TABLE status_page_components
    ADD COLUMN visibility VARCHAR(16) NOT NULL DEFAULT 'enabled',
    ADD COLUMN lifecycle VARCHAR(16) NOT NULL DEFAULT 'active',
    ADD COLUMN archived_at_micros BIGINT;
ALTER TABLE status_page_components
    ADD CONSTRAINT chk_status_page_component_visibility
        CHECK (visibility IN ('enabled', 'hidden')),
    ADD CONSTRAINT chk_status_page_component_lifecycle
        CHECK (lifecycle IN ('active', 'archived')),
    ADD CONSTRAINT chk_status_page_component_archive
        CHECK ((lifecycle = 'archived') = (archived_at_micros IS NOT NULL));
CREATE INDEX idx_status_page_components_management
    ON status_page_components(org_id, status_page_id, lifecycle, visibility, position, name);

-- Drafts can be incomplete and gain their first timeline update atomically
-- when published.
ALTER TABLE status_page_incidents
    RENAME COLUMN resolved_at_micros TO ended_at_micros;
ALTER TABLE status_page_incidents
    ADD COLUMN publication_state VARCHAR(16) NOT NULL,
    ADD COLUMN published_at_micros BIGINT,
    ADD COLUMN draft_message VARCHAR(4000);
ALTER TABLE status_page_incidents
    DROP CONSTRAINT chk_status_page_incident_status,
    DROP CONSTRAINT chk_status_page_incident_resolution,
    DROP CONSTRAINT chk_status_page_incident_kind_impact;
ALTER TABLE status_page_incident_updates
    DROP CONSTRAINT chk_status_page_incident_update_status;

ALTER TABLE status_page_incidents
    ADD CONSTRAINT chk_status_page_incident_status
        CHECK (status IN (
            'investigating', 'identified', 'in_progress', 'monitoring', 'resolved',
            'scheduled', 'completed', 'cancelled'
        )),
    ADD CONSTRAINT chk_status_page_incident_publication
        CHECK (
            (publication_state = 'draft' AND published_at_micros IS NULL)
            OR (publication_state = 'published' AND published_at_micros IS NOT NULL
                AND draft_message IS NULL)
        ),
    ADD CONSTRAINT chk_status_page_incident_draft_message
        CHECK (draft_message IS NULL OR length(btrim(draft_message)) BETWEEN 1 AND 4000),
    ADD CONSTRAINT chk_status_page_incident_terminal
        CHECK (
            (kind = 'incident'
             AND status IN ('investigating', 'identified', 'in_progress', 'monitoring', 'resolved')
             AND ((status = 'resolved') = (ended_at_micros IS NOT NULL)))
            OR
            (kind = 'maintenance'
             AND status IN ('scheduled', 'in_progress', 'completed', 'cancelled')
             AND ((status IN ('completed', 'cancelled')) = (ended_at_micros IS NOT NULL)))
        ),
    ADD CONSTRAINT chk_status_page_incident_kind_impact
        CHECK (
            (kind = 'incident' AND impact <> 'maintenance')
            OR (kind = 'maintenance' AND impact = 'maintenance' AND source_incident_id IS NULL)
        );
ALTER TABLE status_page_incident_updates
    ADD CONSTRAINT chk_status_page_incident_update_status
        CHECK (status IN (
            'investigating', 'identified', 'in_progress', 'monitoring', 'resolved',
            'scheduled', 'completed', 'cancelled'
        ));
DROP INDEX idx_status_page_incidents_active;
DROP INDEX idx_status_page_incidents_resolved;
CREATE INDEX idx_status_page_incidents_working_set
    ON status_page_incidents(
        org_id, status_page_id, kind, publication_state, status, started_at_micros DESC
    );
CREATE INDEX idx_status_page_incidents_history
    ON status_page_incidents(org_id, status_page_id, ended_at_micros DESC, id DESC)
    WHERE publication_state = 'published' AND ended_at_micros IS NOT NULL;
CREATE INDEX idx_status_page_incident_updates_search
    ON status_page_incident_updates(org_id, status_page_id, incident_id, lower(message));

-- Bind a Status Page to one globally reserved ACME domain. The Status Page
-- workflow is intentionally not license-gated even though the generic domain
-- management screen remains a separately licensed capability.
ALTER TABLE domains
    ADD CONSTRAINT uq_domains_org_id_id UNIQUE (org_id, id);
CREATE TABLE status_page_domain_configs (
    org_id                      VARCHAR(64)  NOT NULL,
    status_page_id              VARCHAR(64)  NOT NULL,
    hostname                    VARCHAR(253) NOT NULL,
    verification_token          VARCHAR(160) NOT NULL,
    state                       VARCHAR(32)  NOT NULL DEFAULT 'pending_dns',
    domain_id                   VARCHAR(64),
    routing_valid               BOOLEAN      NOT NULL DEFAULT FALSE,
    last_checked_at_micros      BIGINT,
    last_error                  VARCHAR(500),
    last_alerted_state          VARCHAR(32),
    last_alerted_at_micros      BIGINT,
    created_at_micros           BIGINT       NOT NULL,
    updated_at_micros           BIGINT       NOT NULL,
    PRIMARY KEY (org_id, status_page_id),
    CONSTRAINT fk_status_page_domain_page
        FOREIGN KEY (org_id, status_page_id)
        REFERENCES status_pages(org_id, id) ON DELETE CASCADE,
    CONSTRAINT fk_status_page_domain_acme
        FOREIGN KEY (org_id, domain_id)
        REFERENCES domains(org_id, id) ON DELETE SET NULL,
    CONSTRAINT uq_status_page_domain_hostname UNIQUE (hostname),
    CONSTRAINT uq_status_page_domain_id UNIQUE (domain_id),
    CONSTRAINT chk_status_page_domain_state
        CHECK (state IN (
            'pending_dns', 'verifying', 'verified', 'provisioning_tls',
            'active', 'failed', 'degraded'
        )),
    CONSTRAINT chk_status_page_domain_alert_state
        CHECK (last_alerted_state IS NULL OR last_alerted_state IN ('failed', 'degraded'))
);
CREATE INDEX idx_status_page_domains_due
    ON status_page_domain_configs(last_checked_at_micros, updated_at_micros);

-- Access rules and visitor identities are encrypted with the same tenant-aware
-- envelope used by subscribers. Only hashes participate in lookups.
CREATE TABLE status_page_access_rules (
    id                  VARCHAR(64) PRIMARY KEY,
    org_id              VARCHAR(64) NOT NULL,
    status_page_id      VARCHAR(64) NOT NULL,
    kind                VARCHAR(16) NOT NULL,
    value_ciphertext    BYTEA       NOT NULL,
    value_nonce         BYTEA       NOT NULL,
    value_hash          CHAR(64)    NOT NULL,
    created_at_micros   BIGINT      NOT NULL,
    updated_at_micros   BIGINT      NOT NULL,
    CONSTRAINT uq_status_page_access_rules_scope UNIQUE (org_id, status_page_id, id),
    CONSTRAINT fk_status_page_access_rule_page
        FOREIGN KEY (org_id, status_page_id)
        REFERENCES status_pages(org_id, id) ON DELETE CASCADE,
    CONSTRAINT uq_status_page_access_rule_value
        UNIQUE (org_id, status_page_id, kind, value_hash),
    CONSTRAINT chk_status_page_access_rule_kind CHECK (kind IN ('email', 'domain')),
    CONSTRAINT chk_status_page_access_rule_cipher
        CHECK (octet_length(value_nonce) = 12 AND octet_length(value_ciphertext) >= 17)
);

CREATE TABLE status_page_magic_links (
    id                      VARCHAR(64) PRIMARY KEY,
    org_id                  VARCHAR(64) NOT NULL,
    status_page_id          VARCHAR(64) NOT NULL,
    access_rule_id          VARCHAR(64) NOT NULL,
    email_ciphertext        BYTEA       NOT NULL,
    email_nonce             BYTEA       NOT NULL,
    email_hash              CHAR(64)    NOT NULL,
    token_hash              CHAR(64)    NOT NULL UNIQUE,
    origin_host             VARCHAR(253) NOT NULL,
    expires_at_micros       BIGINT      NOT NULL,
    consumed_at_micros      BIGINT,
    created_at_micros       BIGINT      NOT NULL,
    CONSTRAINT fk_status_page_magic_link_rule
        FOREIGN KEY (org_id, status_page_id, access_rule_id)
        REFERENCES status_page_access_rules(org_id, status_page_id, id) ON DELETE CASCADE,
    CONSTRAINT chk_status_page_magic_link_cipher
        CHECK (octet_length(email_nonce) = 12 AND octet_length(email_ciphertext) >= 17),
    CONSTRAINT chk_status_page_magic_link_expiry
        CHECK (expires_at_micros > created_at_micros)
);
CREATE INDEX idx_status_page_magic_links_expiry
    ON status_page_magic_links(expires_at_micros)
    WHERE consumed_at_micros IS NULL;
CREATE INDEX idx_status_page_magic_links_rate_limit
    ON status_page_magic_links(org_id, status_page_id, email_hash, created_at_micros DESC);
CREATE INDEX idx_status_page_magic_links_page_rate_limit
    ON status_page_magic_links(org_id, status_page_id, created_at_micros DESC);

CREATE TABLE status_page_access_sessions (
    id                      VARCHAR(64) PRIMARY KEY,
    org_id                  VARCHAR(64) NOT NULL,
    status_page_id          VARCHAR(64) NOT NULL,
    access_rule_id          VARCHAR(64) NOT NULL,
    email_ciphertext        BYTEA       NOT NULL,
    email_nonce             BYTEA       NOT NULL,
    email_hash              CHAR(64)    NOT NULL,
    session_token_hash      CHAR(64)    NOT NULL UNIQUE,
    origin_host             VARCHAR(253) NOT NULL,
    expires_at_micros       BIGINT      NOT NULL,
    revoked_at_micros       BIGINT,
    last_seen_at_micros     BIGINT      NOT NULL,
    created_at_micros       BIGINT      NOT NULL,
    CONSTRAINT uq_status_page_access_sessions_scope UNIQUE (org_id, status_page_id, id),
    CONSTRAINT fk_status_page_access_session_rule
        FOREIGN KEY (org_id, status_page_id, access_rule_id)
        REFERENCES status_page_access_rules(org_id, status_page_id, id) ON DELETE CASCADE,
    CONSTRAINT chk_status_page_access_session_cipher
        CHECK (octet_length(email_nonce) = 12 AND octet_length(email_ciphertext) >= 17),
    CONSTRAINT chk_status_page_access_session_expiry
        CHECK (expires_at_micros > created_at_micros)
);
CREATE INDEX idx_status_page_access_sessions_management
    ON status_page_access_sessions(org_id, status_page_id, revoked_at_micros, expires_at_micros DESC);
CREATE INDEX idx_status_page_access_sessions_expiry
    ON status_page_access_sessions(expires_at_micros)
    WHERE revoked_at_micros IS NULL;

CREATE INDEX idx_status_page_deliveries_management
    ON status_page_notification_deliveries(
        org_id, status_page_id, created_at_micros DESC, id DESC
    );
CREATE INDEX idx_status_page_deliveries_retention
    ON status_page_notification_deliveries(status, updated_at_micros)
    WHERE status IN ('delivered', 'failed');

-- Rebuild outbox triggers so drafts, initial component baselines and archived
-- pages stay silent. Private pages may notify their verified email subscribers.
CREATE OR REPLACE FUNCTION enqueue_status_page_component_notification()
RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO status_page_notification_deliveries (
        id, org_id, status_page_id, subscriber_id, event_key, payload,
        next_attempt_at_micros, created_at_micros, updated_at_micros
    )
    SELECT
        md5(subscriber.id || ':component_status:' || NEW.id),
        NEW.org_id,
        NEW.status_page_id,
        subscriber.id,
        'component_status:' || NEW.id,
        jsonb_build_object(
            'type', 'component_status_changed',
            'page_name', page.name,
            'page_slug', page.slug,
            'component_id', component.id,
            'component_name', component.name,
            'status', NEW.status,
            'occurred_at', NEW.started_at_micros
        ),
        NEW.started_at_micros,
        NEW.created_at_micros,
        NEW.created_at_micros
    FROM status_page_subscribers subscriber
    JOIN status_pages page
      ON page.org_id = NEW.org_id AND page.id = NEW.status_page_id
    JOIN status_page_components component
      ON component.org_id = NEW.org_id
     AND component.status_page_id = NEW.status_page_id
     AND component.id = NEW.component_id
    WHERE subscriber.org_id = NEW.org_id
      AND subscriber.status_page_id = NEW.status_page_id
      AND subscriber.status = 'active'
      AND page.lifecycle = 'active'
      AND (page.visibility = 'public' OR subscriber.channel = 'email')
      AND component.lifecycle = 'active'
      AND EXISTS (
          SELECT 1
          FROM status_page_component_status_events previous
          WHERE previous.org_id = NEW.org_id
            AND previous.status_page_id = NEW.status_page_id
            AND previous.component_id = NEW.component_id
            AND previous.id <> NEW.id
      )
    ON CONFLICT (subscriber_id, event_key) DO NOTHING;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION enqueue_status_page_incident_notification()
RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO status_page_notification_deliveries (
        id, org_id, status_page_id, subscriber_id, event_key, payload,
        next_attempt_at_micros, created_at_micros, updated_at_micros
    )
    SELECT
        md5(subscriber.id || ':incident_update:' || NEW.id),
        NEW.org_id,
        NEW.status_page_id,
        subscriber.id,
        'incident_update:' || NEW.id,
        jsonb_build_object(
            'type', 'incident_update',
            'page_name', page.name,
            'page_slug', page.slug,
            'incident_id', incident.id,
            'incident_kind', incident.kind,
            'title', incident.title,
            'impact', incident.impact,
            'status', NEW.status,
            'message', NEW.message,
            'occurred_at', NEW.created_at_micros,
            'component_names', COALESCE((
                SELECT jsonb_agg(component.name ORDER BY component.position, component.name)
                FROM status_page_incident_components link
                JOIN status_page_components component
                  ON component.org_id = link.org_id
                 AND component.status_page_id = link.status_page_id
                 AND component.id = link.component_id
                WHERE link.org_id = NEW.org_id
                  AND link.status_page_id = NEW.status_page_id
                  AND link.incident_id = NEW.incident_id
            ), '[]'::JSONB)
        ),
        NEW.created_at_micros,
        NEW.created_at_micros,
        NEW.created_at_micros
    FROM status_page_subscribers subscriber
    JOIN status_pages page
      ON page.org_id = NEW.org_id AND page.id = NEW.status_page_id
    JOIN status_page_incidents incident
      ON incident.org_id = NEW.org_id
     AND incident.status_page_id = NEW.status_page_id
     AND incident.id = NEW.incident_id
    WHERE subscriber.org_id = NEW.org_id
      AND subscriber.status_page_id = NEW.status_page_id
      AND subscriber.status = 'active'
      AND page.lifecycle = 'active'
      AND (page.visibility = 'public' OR subscriber.channel = 'email')
      AND incident.publication_state = 'published'
    ON CONFLICT (subscriber_id, event_key) DO NOTHING;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- ============================================================
-- Synthetic monitoring
-- ============================================================

-- SPDX-License-Identifier: Apache-2.0
-- Copyright (c) 2026 MoleSignal Authors

-- User-facing synthetic monitoring is OSS. Permissions are deliberately split
-- between ordinary monitor management, private execution infrastructure, and
-- write-only authentication material.
INSERT INTO iam_permissions (
    permission_key, scope, domain, label_key, description_key, feature, catalog_version
)
VALUES
    ('synthetics.read', 'organization', 'synthetics',
     'permissions.synthetics_read', 'permissions_hint.synthetics_read', NULL, 9),
    ('synthetics.manage', 'organization', 'synthetics',
     'permissions.synthetics_manage', 'permissions_hint.synthetics_manage', NULL, 9),
    ('synthetics.locations.manage', 'organization', 'synthetics',
     'permissions.synthetics_locations_manage',
     'permissions_hint.synthetics_locations_manage', NULL, 9),
    ('synthetics.secrets.manage', 'organization', 'synthetics',
     'permissions.synthetics_secrets_manage',
     'permissions_hint.synthetics_secrets_manage', NULL, 9),
    ('status_pages.publish', 'organization', 'status_pages',
     'permissions.status_pages_publish', 'permissions_hint.status_pages_publish', NULL, 9),
    ('service_levels.read', 'organization', 'service_levels',
     'permissions.service_levels_read', 'permissions_hint.service_levels_read', NULL, 9),
    ('service_levels.manage', 'organization', 'service_levels',
     'permissions.service_levels_manage', 'permissions_hint.service_levels_manage', NULL, 9),
    ('postmortems.read', 'organization', 'postmortems',
     'permissions.postmortems_read', 'permissions_hint.postmortems_read', NULL, 9),
    ('postmortems.manage', 'organization', 'postmortems',
     'permissions.postmortems_manage', 'permissions_hint.postmortems_manage', NULL, 9)
ON CONFLICT (permission_key) DO UPDATE
SET scope = EXCLUDED.scope,
    domain = EXCLUDED.domain,
    label_key = EXCLUDED.label_key,
    description_key = EXCLUDED.description_key,
    feature = EXCLUDED.feature,
    catalog_version = EXCLUDED.catalog_version;

UPDATE iam_permission_catalog_versions
SET version = GREATEST(version, 9),
    updated_at_micros = (EXTRACT(EPOCH FROM clock_timestamp()) * 1000000)::BIGINT
WHERE catalog_key = 'permissions';

INSERT INTO iam_builtin_role_permissions (role_key, permission_key)
SELECT role_key, permission_key
FROM (VALUES ('owner'), ('admin')) AS roles(role_key)
CROSS JOIN (VALUES
    ('synthetics.read'),
    ('synthetics.manage'),
    ('synthetics.locations.manage'),
    ('synthetics.secrets.manage'),
    ('status_pages.publish'),
    ('service_levels.read'),
    ('service_levels.manage'),
    ('postmortems.read'),
    ('postmortems.manage')
) AS permissions(permission_key)
ON CONFLICT DO NOTHING;

INSERT INTO iam_role_permissions (role_id, permission_key)
SELECT role.id, permission.permission_key
FROM iam_roles role
CROSS JOIN (VALUES
    ('owner', 'synthetics.read'),
    ('owner', 'synthetics.manage'),
    ('owner', 'synthetics.locations.manage'),
    ('owner', 'synthetics.secrets.manage'),
    ('owner', 'status_pages.publish'),
    ('owner', 'service_levels.read'),
    ('owner', 'service_levels.manage'),
    ('owner', 'postmortems.read'),
    ('owner', 'postmortems.manage'),
    ('admin', 'synthetics.read'),
    ('admin', 'synthetics.manage'),
    ('admin', 'synthetics.locations.manage'),
    ('admin', 'synthetics.secrets.manage'),
    ('admin', 'status_pages.publish'),
    ('admin', 'service_levels.read'),
    ('admin', 'service_levels.manage'),
    ('admin', 'postmortems.read'),
    ('admin', 'postmortems.manage')
) AS permission(role_key, permission_key)
WHERE role.builtin AND role.role_key = permission.role_key
ON CONFLICT DO NOTHING;

WITH requested(bundle_key, permission_key, offset_position) AS (
    VALUES
        ('readonly_observer', 'synthetics.read', 1),
        ('readonly_observer', 'service_levels.read', 2),
        ('readonly_observer', 'postmortems.read', 3),
        ('alert_administrator', 'synthetics.read', 1),
        ('alert_administrator', 'synthetics.manage', 2),
        ('alert_administrator', 'status_pages.publish', 3),
        ('alert_administrator', 'postmortems.read', 4),
        ('alert_administrator', 'postmortems.manage', 5),
        ('organization_administrator', 'synthetics.read', 1),
        ('organization_administrator', 'synthetics.manage', 2),
        ('organization_administrator', 'synthetics.locations.manage', 3),
        ('organization_administrator', 'synthetics.secrets.manage', 4),
        ('organization_administrator', 'status_pages.publish', 5),
        ('organization_administrator', 'service_levels.read', 6),
        ('organization_administrator', 'service_levels.manage', 7),
        ('organization_administrator', 'postmortems.read', 8),
        ('organization_administrator', 'postmortems.manage', 9)
), base AS (
    SELECT bundle_key, COALESCE(MAX(position), -1) AS max_position
    FROM iam_permission_bundle_items
    GROUP BY bundle_key
)
INSERT INTO iam_permission_bundle_items (bundle_key, permission_key, position)
SELECT requested.bundle_key,
       requested.permission_key,
       base.max_position + requested.offset_position
FROM requested
JOIN base USING (bundle_key)
ON CONFLICT (bundle_key, permission_key) DO NOTHING;

CREATE TABLE synthetic_probe_locations (
    id                      VARCHAR(64) PRIMARY KEY,
    organization_id         VARCHAR(64) REFERENCES organizations(id) ON DELETE CASCADE,
    name                    VARCHAR(255) NOT NULL,
    code                    VARCHAR(64) NOT NULL,
    description             TEXT NOT NULL DEFAULT '',
    scope                   VARCHAR(16) NOT NULL,
    execution               VARCHAR(16) NOT NULL,
    lifecycle               VARCHAR(16) NOT NULL DEFAULT 'active',
    health                  VARCHAR(16) NOT NULL DEFAULT 'unknown',
    system_managed          BOOLEAN NOT NULL DEFAULT FALSE,
    egress_policy           JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at_micros       BIGINT NOT NULL,
    updated_at_micros       BIGINT NOT NULL,
    CONSTRAINT chk_synthetic_probe_locations_scope
        CHECK (
            (scope = 'platform' AND organization_id IS NULL)
            OR (scope = 'organization' AND organization_id IS NOT NULL)
        ),
    CONSTRAINT chk_synthetic_probe_locations_execution
        CHECK (execution IN ('embedded', 'agent_pool')),
    CONSTRAINT chk_synthetic_probe_locations_lifecycle
        CHECK (lifecycle IN ('active', 'paused', 'archived')),
    CONSTRAINT chk_synthetic_probe_locations_health
        CHECK (health IN ('online', 'degraded', 'offline', 'unknown')),
    CONSTRAINT chk_synthetic_probe_locations_code
        CHECK (code = LOWER(code) AND code ~ '^[a-z0-9]+(?:-[a-z0-9]+)*$'),
    CONSTRAINT chk_synthetic_probe_locations_system_managed
        CHECK (NOT system_managed OR scope = 'platform'),
    CONSTRAINT uq_synthetic_probe_locations_org_id UNIQUE (organization_id, id)
);
CREATE UNIQUE INDEX uq_synthetic_probe_locations_platform_code
    ON synthetic_probe_locations (code)
    WHERE organization_id IS NULL;
CREATE UNIQUE INDEX uq_synthetic_probe_locations_org_code
    ON synthetic_probe_locations (organization_id, code)
    WHERE organization_id IS NOT NULL;
CREATE INDEX ix_synthetic_probe_locations_org_lifecycle
    ON synthetic_probe_locations (organization_id, lifecycle, name);

INSERT INTO synthetic_probe_locations (
    id, organization_id, name, code, description, scope, execution, lifecycle,
    health, system_managed, egress_policy, created_at_micros, updated_at_micros
)
VALUES (
    'builtin-local', NULL, 'Local', 'local',
    'Runs probes inside the MoleSignal process.', 'platform', 'embedded', 'active',
    'online', TRUE,
    '{"allowed_cidrs":[],"denied_cidrs":[],"allowed_domains":[],"denied_domains":[],"allowed_ports":[],"allow_private_networks":false,"allow_loopback":false}'::JSONB,
    (EXTRACT(EPOCH FROM clock_timestamp()) * 1000000)::BIGINT,
    (EXTRACT(EPOCH FROM clock_timestamp()) * 1000000)::BIGINT
);

CREATE TABLE synthetic_probe_certificate_authorities (
    id                      VARCHAR(64) PRIMARY KEY,
    private_key_nonce       BYTEA NOT NULL,
    private_key_ciphertext  BYTEA NOT NULL,
    certificate_pem         TEXT NOT NULL,
    created_at_micros       BIGINT NOT NULL,
    CONSTRAINT chk_synthetic_probe_ca_nonce
        CHECK (OCTET_LENGTH(private_key_nonce) = 12),
    CONSTRAINT chk_synthetic_probe_ca_ciphertext
        CHECK (OCTET_LENGTH(private_key_ciphertext) > 16)
);

CREATE TABLE synthetic_probe_agents (
    id                              VARCHAR(64) PRIMARY KEY,
    location_id                     VARCHAR(64) NOT NULL
                                        REFERENCES synthetic_probe_locations(id) ON DELETE CASCADE,
    name                            VARCHAR(255) NOT NULL,
    hostname                        VARCHAR(255) NOT NULL,
    status                          VARCHAR(16) NOT NULL DEFAULT 'registered',
    agent_version                   VARCHAR(64) NOT NULL,
    protocol_version                INTEGER NOT NULL,
    capabilities                    TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[],
    capacity                        JSONB NOT NULL DEFAULT '{}'::JSONB,
    labels                          JSONB NOT NULL DEFAULT '{}'::JSONB,
    public_key_der                  BYTEA NOT NULL,
    certificate_serial              VARCHAR(128),
    certificate_expires_at_micros   BIGINT,
    last_heartbeat_at_micros        BIGINT,
    last_result_sequence            BIGINT NOT NULL DEFAULT 0,
    revoked_at_micros               BIGINT,
    created_at_micros               BIGINT NOT NULL,
    updated_at_micros               BIGINT NOT NULL,
    CONSTRAINT chk_synthetic_probe_agents_status
        CHECK (status IN ('registered', 'online', 'degraded', 'offline', 'draining', 'revoked')),
    CONSTRAINT chk_synthetic_probe_agents_protocol_version CHECK (protocol_version > 0),
    CONSTRAINT chk_synthetic_probe_agents_result_sequence CHECK (last_result_sequence >= 0),
    CONSTRAINT chk_synthetic_probe_agents_revoked
        CHECK ((status = 'revoked') = (revoked_at_micros IS NOT NULL))
);
CREATE UNIQUE INDEX uq_synthetic_probe_agents_certificate
    ON synthetic_probe_agents (certificate_serial)
    WHERE certificate_serial IS NOT NULL;
CREATE INDEX ix_synthetic_probe_agents_location_status
    ON synthetic_probe_agents (location_id, status, last_heartbeat_at_micros DESC);

CREATE TABLE synthetic_probe_register_tokens (
    id                      VARCHAR(64) PRIMARY KEY,
    location_id             VARCHAR(64) NOT NULL
                                REFERENCES synthetic_probe_locations(id) ON DELETE CASCADE,
    token_hash              BYTEA NOT NULL UNIQUE,
    expires_at_micros       BIGINT NOT NULL,
    used_at_micros          BIGINT,
    used_by_agent_id        VARCHAR(64) REFERENCES synthetic_probe_agents(id),
    created_by              VARCHAR(64) NOT NULL REFERENCES users(id),
    created_at_micros       BIGINT NOT NULL,
    CONSTRAINT chk_synthetic_probe_register_token_use
        CHECK (
            (used_at_micros IS NULL AND used_by_agent_id IS NULL)
            OR (used_at_micros IS NOT NULL AND used_by_agent_id IS NOT NULL)
        )
);
CREATE INDEX ix_synthetic_probe_register_tokens_location_expiry
    ON synthetic_probe_register_tokens (location_id, expires_at_micros)
    WHERE used_at_micros IS NULL;

CREATE TABLE synthetic_probe_agent_tokens (
    id                      VARCHAR(64) PRIMARY KEY,
    organization_id         VARCHAR(64) NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    location_id             VARCHAR(64) NOT NULL,
    name                    VARCHAR(128) NOT NULL,
    token_hash              BYTEA NOT NULL UNIQUE,
    token_prefix            VARCHAR(32) NOT NULL,
    status                  VARCHAR(16) NOT NULL DEFAULT 'active',
    expires_at_micros       BIGINT,
    last_used_at_micros     BIGINT,
    created_by              VARCHAR(64) NOT NULL REFERENCES users(id),
    created_at_micros       BIGINT NOT NULL,
    rotated_at_micros       BIGINT,
    disabled_at_micros      BIGINT,
    updated_at_micros       BIGINT NOT NULL,
    CONSTRAINT fk_synthetic_probe_agent_tokens_location
        FOREIGN KEY (organization_id, location_id)
        REFERENCES synthetic_probe_locations(organization_id, id) ON DELETE CASCADE,
    CONSTRAINT uq_synthetic_probe_agent_tokens_org_id UNIQUE (organization_id, id),
    CONSTRAINT chk_synthetic_probe_agent_tokens_status
        CHECK (status IN ('active', 'disabled')),
    CONSTRAINT chk_synthetic_probe_agent_tokens_disabled
        CHECK ((status = 'disabled') = (disabled_at_micros IS NOT NULL)),
    CONSTRAINT chk_synthetic_probe_agent_tokens_name
        CHECK (name = BTRIM(name) AND name <> ''),
    CONSTRAINT chk_synthetic_probe_agent_tokens_hash
        CHECK (OCTET_LENGTH(token_hash) = 32),
    CONSTRAINT chk_synthetic_probe_agent_tokens_prefix
        CHECK (token_prefix ~ '^ms_probe_[A-Za-z0-9_-]{8}$'),
    CONSTRAINT chk_synthetic_probe_agent_tokens_expiry
        CHECK (expires_at_micros IS NULL OR expires_at_micros > created_at_micros),
    CONSTRAINT chk_synthetic_probe_agent_tokens_timestamps
        CHECK (
            updated_at_micros >= created_at_micros
            AND (last_used_at_micros IS NULL OR last_used_at_micros >= created_at_micros)
            AND (rotated_at_micros IS NULL OR rotated_at_micros >= created_at_micros)
            AND (disabled_at_micros IS NULL OR disabled_at_micros >= created_at_micros)
        )
);
CREATE INDEX ix_synthetic_probe_agent_tokens_org_status
    ON synthetic_probe_agent_tokens (organization_id, status, name, id);

CREATE TABLE synthetic_secrets (
    id                      VARCHAR(64) PRIMARY KEY,
    organization_id         VARCHAR(64) NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name                    VARCHAR(255) NOT NULL,
    description             TEXT NOT NULL DEFAULT '',
    current_version         INTEGER NOT NULL DEFAULT 1,
    created_by              VARCHAR(64) NOT NULL REFERENCES users(id),
    created_at_micros       BIGINT NOT NULL,
    updated_at_micros       BIGINT NOT NULL,
    archived_at_micros      BIGINT,
    CONSTRAINT uq_synthetic_secrets_org_name UNIQUE (organization_id, name),
    CONSTRAINT uq_synthetic_secrets_org_id UNIQUE (organization_id, id),
    CONSTRAINT chk_synthetic_secrets_version CHECK (current_version > 0)
);

CREATE TABLE synthetic_secret_versions (
    secret_id               VARCHAR(64) NOT NULL,
    organization_id         VARCHAR(64) NOT NULL,
    version                 INTEGER NOT NULL,
    nonce                   BYTEA NOT NULL,
    ciphertext              BYTEA NOT NULL,
    created_by              VARCHAR(64) NOT NULL REFERENCES users(id),
    created_at_micros       BIGINT NOT NULL,
    PRIMARY KEY (secret_id, version),
    CONSTRAINT fk_synthetic_secret_versions_secret
        FOREIGN KEY (organization_id, secret_id)
        REFERENCES synthetic_secrets(organization_id, id) ON DELETE CASCADE,
    CONSTRAINT chk_synthetic_secret_versions_version CHECK (version > 0),
    CONSTRAINT chk_synthetic_secret_versions_nonce CHECK (OCTET_LENGTH(nonce) = 12),
    CONSTRAINT chk_synthetic_secret_versions_ciphertext CHECK (OCTET_LENGTH(ciphertext) > 16)
);

CREATE TABLE synthetic_monitors (
    id                      VARCHAR(64) PRIMARY KEY,
    organization_id         VARCHAR(64) NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name                    VARCHAR(255) NOT NULL,
    description             TEXT NOT NULL DEFAULT '',
    kind                    VARCHAR(16) NOT NULL,
    lifecycle               VARCHAR(16) NOT NULL DEFAULT 'draft',
    state                   VARCHAR(16) NOT NULL DEFAULT 'unknown',
    team_id                 VARCHAR(64),
    tags                    TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[],
    active_revision_id      VARCHAR(64),
    draft_revision_id       VARCHAR(64),
    next_due_at_micros      BIGINT,
    created_by              VARCHAR(64) NOT NULL REFERENCES users(id),
    created_at_micros       BIGINT NOT NULL,
    updated_at_micros       BIGINT NOT NULL,
    archived_at_micros      BIGINT,
    CONSTRAINT uq_synthetic_monitors_org_id UNIQUE (organization_id, id),
    CONSTRAINT chk_synthetic_monitors_kind
        CHECK (kind IN ('http', 'tcp', 'ssh', 'dns', 'icmp', 'tls', 'grpc', 'browser', 'heartbeat')),
    CONSTRAINT chk_synthetic_monitors_lifecycle
        CHECK (lifecycle IN ('draft', 'active', 'paused', 'archived')),
    CONSTRAINT chk_synthetic_monitors_state
        CHECK (state IN ('healthy', 'flaky', 'degraded', 'failing', 'unknown')),
    CONSTRAINT chk_synthetic_monitors_archive
        CHECK ((lifecycle = 'archived') = (archived_at_micros IS NOT NULL))
);
CREATE UNIQUE INDEX uq_synthetic_monitors_org_name_active
    ON synthetic_monitors (organization_id, LOWER(name))
    WHERE lifecycle <> 'archived';
CREATE INDEX ix_synthetic_monitors_org_state
    ON synthetic_monitors (organization_id, lifecycle, state, updated_at_micros DESC);

CREATE TABLE synthetic_monitor_revisions (
    id                          VARCHAR(64) PRIMARY KEY,
    organization_id             VARCHAR(64) NOT NULL,
    monitor_id                  VARCHAR(64) NOT NULL,
    revision_number             INTEGER NOT NULL,
    spec                        JSONB NOT NULL,
    schedule                    JSONB NOT NULL,
    timeout_millis              INTEGER NOT NULL,
    max_retries                 SMALLINT NOT NULL DEFAULT 0,
    consecutive_failures        INTEGER NOT NULL DEFAULT 3,
    consecutive_recoveries      INTEGER NOT NULL DEFAULT 2,
    freshness_seconds           INTEGER NOT NULL,
    location_policy             JSONB NOT NULL,
    escalation_policy_id        VARCHAR(64) REFERENCES escalation_policies(id) ON DELETE SET NULL,
    alert_on_degraded            BOOLEAN NOT NULL DEFAULT FALSE,
    alert_on_flaky               BOOLEAN NOT NULL DEFAULT FALSE,
    last_test_result_id          VARCHAR(64),
    last_test_passed_at_micros   BIGINT,
    heartbeat_token_hash         BYTEA,
    created_by                  VARCHAR(64) NOT NULL REFERENCES users(id),
    created_at_micros           BIGINT NOT NULL,
    content_hash                VARCHAR(64) NOT NULL,
    CONSTRAINT fk_synthetic_monitor_revisions_monitor
        FOREIGN KEY (organization_id, monitor_id)
        REFERENCES synthetic_monitors(organization_id, id) ON DELETE CASCADE,
    CONSTRAINT uq_synthetic_monitor_revisions_number UNIQUE (monitor_id, revision_number),
    CONSTRAINT uq_synthetic_monitor_revisions_org_id UNIQUE (organization_id, id),
    CONSTRAINT chk_synthetic_monitor_revisions_number CHECK (revision_number > 0),
    CONSTRAINT chk_synthetic_monitor_revisions_timeout
        CHECK (timeout_millis BETWEEN 100 AND 300000),
    CONSTRAINT chk_synthetic_monitor_revisions_retries CHECK (max_retries BETWEEN 0 AND 2),
    CONSTRAINT chk_synthetic_monitor_revisions_failure_threshold
        CHECK (consecutive_failures BETWEEN 1 AND 100),
    CONSTRAINT chk_synthetic_monitor_revisions_recovery_threshold
        CHECK (consecutive_recoveries BETWEEN 1 AND 100),
    CONSTRAINT chk_synthetic_monitor_revisions_freshness CHECK (freshness_seconds > 0)
);

ALTER TABLE synthetic_monitors
    ADD CONSTRAINT fk_synthetic_monitors_active_revision
        FOREIGN KEY (organization_id, active_revision_id)
        REFERENCES synthetic_monitor_revisions(organization_id, id)
        DEFERRABLE INITIALLY DEFERRED,
    ADD CONSTRAINT fk_synthetic_monitors_draft_revision
        FOREIGN KEY (organization_id, draft_revision_id)
        REFERENCES synthetic_monitor_revisions(organization_id, id)
        DEFERRABLE INITIALLY DEFERRED;

CREATE TABLE synthetic_monitor_revision_locations (
    organization_id         VARCHAR(64) NOT NULL,
    revision_id             VARCHAR(64) NOT NULL,
    location_id             VARCHAR(64) NOT NULL REFERENCES synthetic_probe_locations(id),
    position                INTEGER NOT NULL,
    PRIMARY KEY (revision_id, location_id),
    CONSTRAINT fk_synthetic_monitor_revision_locations_revision
        FOREIGN KEY (organization_id, revision_id)
        REFERENCES synthetic_monitor_revisions(organization_id, id) ON DELETE CASCADE,
    CONSTRAINT chk_synthetic_monitor_revision_locations_position CHECK (position >= 0)
);
CREATE INDEX ix_synthetic_monitor_revision_locations_location
    ON synthetic_monitor_revision_locations (location_id, revision_id);

CREATE TABLE synthetic_probe_tasks (
    id                      VARCHAR(64) PRIMARY KEY,
    organization_id         VARCHAR(64) NOT NULL,
    monitor_id              VARCHAR(64) NOT NULL,
    monitor_revision_id     VARCHAR(64) NOT NULL,
    location_id             VARCHAR(64) NOT NULL REFERENCES synthetic_probe_locations(id),
    spec                    JSONB NOT NULL,
    is_test                 BOOLEAN NOT NULL DEFAULT FALSE,
    state                   VARCHAR(16) NOT NULL DEFAULT 'pending',
    scheduled_at_micros     BIGINT NOT NULL,
    deadline_at_micros      BIGINT NOT NULL,
    timeout_millis          INTEGER NOT NULL,
    max_attempts            SMALLINT NOT NULL,
    lease_agent_id          VARCHAR(64) REFERENCES synthetic_probe_agents(id),
    lease_token_hash        VARCHAR(128),
    leased_until_micros     BIGINT,
    created_at_micros       BIGINT NOT NULL,
    updated_at_micros       BIGINT NOT NULL,
    CONSTRAINT fk_synthetic_probe_tasks_monitor
        FOREIGN KEY (organization_id, monitor_id)
        REFERENCES synthetic_monitors(organization_id, id) ON DELETE CASCADE,
    CONSTRAINT fk_synthetic_probe_tasks_revision
        FOREIGN KEY (organization_id, monitor_revision_id)
        REFERENCES synthetic_monitor_revisions(organization_id, id),
    CONSTRAINT uq_synthetic_probe_tasks_schedule
        UNIQUE (monitor_revision_id, location_id, scheduled_at_micros, is_test),
    CONSTRAINT chk_synthetic_probe_tasks_state
        CHECK (state IN ('pending', 'leased', 'running', 'completed', 'cancelled',
                         'expired', 'skipped')),
    CONSTRAINT chk_synthetic_probe_tasks_window
        CHECK (deadline_at_micros > scheduled_at_micros),
    CONSTRAINT chk_synthetic_probe_tasks_attempts CHECK (max_attempts BETWEEN 1 AND 3),
    CONSTRAINT chk_synthetic_probe_tasks_lease
        CHECK (
            (lease_agent_id IS NULL AND lease_token_hash IS NULL AND leased_until_micros IS NULL)
            OR
            (lease_agent_id IS NOT NULL AND lease_token_hash IS NOT NULL
             AND leased_until_micros IS NOT NULL)
        )
);
CREATE INDEX ix_synthetic_probe_tasks_dispatch
    ON synthetic_probe_tasks (location_id, state, scheduled_at_micros)
    WHERE state = 'pending';
CREATE INDEX ix_synthetic_probe_tasks_expired_lease
    ON synthetic_probe_tasks (leased_until_micros)
    WHERE state IN ('leased', 'running');

CREATE TABLE synthetic_results (
    id                          VARCHAR(64) PRIMARY KEY,
    organization_id             VARCHAR(64) NOT NULL,
    monitor_id                  VARCHAR(64) NOT NULL,
    monitor_revision_id         VARCHAR(64) NOT NULL,
    location_id                 VARCHAR(64) NOT NULL REFERENCES synthetic_probe_locations(id),
    agent_id                    VARCHAR(64) REFERENCES synthetic_probe_agents(id),
    task_id                     VARCHAR(64) NOT NULL REFERENCES synthetic_probe_tasks(id),
    is_test                     BOOLEAN NOT NULL DEFAULT FALSE,
    result_sequence             BIGINT,
    scheduled_at_micros         BIGINT NOT NULL,
    started_at_micros           BIGINT NOT NULL,
    finished_at_micros          BIGINT NOT NULL,
    received_at_micros          BIGINT NOT NULL,
    outcome                     VARCHAR(16) NOT NULL,
    attempts                    JSONB NOT NULL DEFAULT '[]'::JSONB,
    assertions                  JSONB NOT NULL DEFAULT '[]'::JSONB,
    secret_versions             JSONB NOT NULL DEFAULT '{}'::JSONB,
    protocol_version            INTEGER NOT NULL,
    metadata                    JSONB NOT NULL DEFAULT '{}'::JSONB,
    detail_object_key           TEXT,
    CONSTRAINT fk_synthetic_results_monitor
        FOREIGN KEY (organization_id, monitor_id)
        REFERENCES synthetic_monitors(organization_id, id) ON DELETE CASCADE,
    CONSTRAINT fk_synthetic_results_revision
        FOREIGN KEY (organization_id, monitor_revision_id)
        REFERENCES synthetic_monitor_revisions(organization_id, id),
    CONSTRAINT uq_synthetic_results_org_id UNIQUE (organization_id, id),
    CONSTRAINT uq_synthetic_results_task UNIQUE (task_id),
    CONSTRAINT chk_synthetic_results_outcome
        CHECK (outcome IN ('healthy', 'flaky', 'degraded', 'failing', 'unknown', 'skipped')),
    CONSTRAINT chk_synthetic_results_window
        CHECK (started_at_micros <= finished_at_micros
               AND finished_at_micros <= received_at_micros),
    CONSTRAINT chk_synthetic_results_sequence
        CHECK (result_sequence IS NULL OR result_sequence >= 0)
);
CREATE UNIQUE INDEX uq_synthetic_results_agent_sequence
    ON synthetic_results (agent_id, result_sequence)
    WHERE agent_id IS NOT NULL AND result_sequence IS NOT NULL;
CREATE INDEX ix_synthetic_results_monitor_time
    ON synthetic_results (organization_id, monitor_id, finished_at_micros DESC, id DESC);
CREATE INDEX ix_synthetic_results_retention
    ON synthetic_results (organization_id, received_at_micros);

ALTER TABLE synthetic_monitor_revisions
    ADD CONSTRAINT fk_synthetic_monitor_revisions_last_test_result
        FOREIGN KEY (last_test_result_id) REFERENCES synthetic_results(id)
        ON DELETE SET NULL DEFERRABLE INITIALLY DEFERRED,
    ADD CONSTRAINT chk_synthetic_monitor_revisions_last_test
        CHECK (
            (last_test_result_id IS NULL AND last_test_passed_at_micros IS NULL)
            OR (last_test_result_id IS NOT NULL AND last_test_passed_at_micros IS NOT NULL)
        );

CREATE TABLE synthetic_monitor_location_states (
    organization_id             VARCHAR(64) NOT NULL,
    monitor_id                  VARCHAR(64) NOT NULL,
    monitor_revision_id         VARCHAR(64) NOT NULL,
    location_id                 VARCHAR(64) NOT NULL REFERENCES synthetic_probe_locations(id),
    current_state               VARCHAR(16) NOT NULL DEFAULT 'unknown',
    candidate_state             VARCHAR(16),
    candidate_count             INTEGER NOT NULL DEFAULT 0,
    last_result_id              VARCHAR(64) REFERENCES synthetic_results(id),
    last_observed_at_micros     BIGINT,
    updated_at_micros           BIGINT NOT NULL,
    PRIMARY KEY (organization_id, monitor_id, monitor_revision_id, location_id),
    CONSTRAINT fk_synthetic_monitor_location_states_monitor
        FOREIGN KEY (organization_id, monitor_id)
        REFERENCES synthetic_monitors(organization_id, id) ON DELETE CASCADE,
    CONSTRAINT fk_synthetic_monitor_location_states_revision
        FOREIGN KEY (organization_id, monitor_revision_id)
        REFERENCES synthetic_monitor_revisions(organization_id, id) ON DELETE CASCADE,
    CONSTRAINT chk_synthetic_monitor_location_states_current
        CHECK (current_state IN ('healthy', 'flaky', 'degraded', 'failing', 'unknown')),
    CONSTRAINT chk_synthetic_monitor_location_states_candidate
        CHECK (candidate_state IS NULL OR candidate_state IN
            ('healthy', 'flaky', 'degraded', 'failing', 'unknown')),
    CONSTRAINT chk_synthetic_monitor_location_states_count CHECK (candidate_count >= 0)
);
CREATE INDEX ix_synthetic_monitor_location_states_monitor
    ON synthetic_monitor_location_states
       (organization_id, monitor_id, monitor_revision_id, current_state);

CREATE TABLE synthetic_state_transitions (
    id                          VARCHAR(64) PRIMARY KEY,
    organization_id             VARCHAR(64) NOT NULL,
    monitor_id                  VARCHAR(64) NOT NULL,
    monitor_revision_id         VARCHAR(64) NOT NULL,
    location_id                 VARCHAR(64) REFERENCES synthetic_probe_locations(id),
    previous_state              VARCHAR(16) NOT NULL,
    current_state               VARCHAR(16) NOT NULL,
    correlation_key             VARCHAR(255) NOT NULL,
    result_id                   VARCHAR(64) REFERENCES synthetic_results(id),
    started_at_micros           BIGINT NOT NULL,
    ended_at_micros             BIGINT,
    created_at_micros           BIGINT NOT NULL,
    CONSTRAINT fk_synthetic_state_transitions_monitor
        FOREIGN KEY (organization_id, monitor_id)
        REFERENCES synthetic_monitors(organization_id, id) ON DELETE CASCADE,
    CONSTRAINT fk_synthetic_state_transitions_revision
        FOREIGN KEY (organization_id, monitor_revision_id)
        REFERENCES synthetic_monitor_revisions(organization_id, id),
    CONSTRAINT chk_synthetic_state_transitions_previous
        CHECK (previous_state IN ('healthy', 'flaky', 'degraded', 'failing', 'unknown')),
    CONSTRAINT chk_synthetic_state_transitions_current
        CHECK (current_state IN ('healthy', 'flaky', 'degraded', 'failing', 'unknown')),
    CONSTRAINT chk_synthetic_state_transitions_change CHECK (previous_state <> current_state),
    CONSTRAINT chk_synthetic_state_transitions_window
        CHECK (ended_at_micros IS NULL OR ended_at_micros >= started_at_micros)
);
CREATE UNIQUE INDEX uq_synthetic_state_transitions_open
    ON synthetic_state_transitions (organization_id, monitor_id, COALESCE(location_id, ''))
    WHERE ended_at_micros IS NULL;
CREATE INDEX ix_synthetic_state_transitions_monitor_time
    ON synthetic_state_transitions (organization_id, monitor_id, started_at_micros DESC);

CREATE TABLE synthetic_result_artifacts (
    id                      VARCHAR(64) PRIMARY KEY,
    organization_id         VARCHAR(64) NOT NULL,
    result_id               VARCHAR(64) NOT NULL,
    kind                    VARCHAR(32) NOT NULL,
    name                    VARCHAR(128) NOT NULL DEFAULT '',
    object_key              TEXT NOT NULL,
    content_type            VARCHAR(128) NOT NULL,
    content_length          BIGINT NOT NULL,
    sha256                  BYTEA NOT NULL,
    expires_at_micros       BIGINT NOT NULL,
    created_at_micros       BIGINT NOT NULL,
    CONSTRAINT fk_synthetic_result_artifacts_result
        FOREIGN KEY (organization_id, result_id)
        REFERENCES synthetic_results(organization_id, id) ON DELETE CASCADE,
    CONSTRAINT chk_synthetic_result_artifacts_length CHECK (content_length >= 0),
    CONSTRAINT chk_synthetic_result_artifacts_sha CHECK (OCTET_LENGTH(sha256) = 32),
    CONSTRAINT uq_synthetic_result_artifacts_object UNIQUE (object_key)
);
CREATE INDEX ix_synthetic_result_artifacts_retention
    ON synthetic_result_artifacts (organization_id, expires_at_micros);

CREATE TABLE synthetic_retention_settings (
    organization_id         VARCHAR(64) PRIMARY KEY REFERENCES organizations(id) ON DELETE CASCADE,
    raw_result_days         INTEGER NOT NULL DEFAULT 30,
    artifact_days           INTEGER NOT NULL DEFAULT 7,
    updated_by              VARCHAR(64) NOT NULL REFERENCES users(id),
    updated_at_micros       BIGINT NOT NULL,
    CONSTRAINT chk_synthetic_retention_raw CHECK (raw_result_days BETWEEN 1 AND 365),
    CONSTRAINT chk_synthetic_retention_artifacts CHECK (artifact_days BETWEEN 1 AND 30)
);

-- Ordered, immutable Status Page automation rules and durable publication work.
CREATE TABLE status_page_automation_rules (
    id                      VARCHAR(64) PRIMARY KEY,
    organization_id         VARCHAR(64) NOT NULL,
    status_page_id          VARCHAR(64) NOT NULL,
    name                    VARCHAR(255) NOT NULL,
    position                INTEGER NOT NULL,
    lifecycle               VARCHAR(16) NOT NULL DEFAULT 'draft',
    active_revision_id      VARCHAR(64),
    draft_revision_id       VARCHAR(64),
    created_by              VARCHAR(64) NOT NULL REFERENCES users(id),
    created_at_micros       BIGINT NOT NULL,
    updated_at_micros       BIGINT NOT NULL,
    CONSTRAINT uq_status_page_automation_rules_org_id UNIQUE (organization_id, id),
    CONSTRAINT fk_status_page_automation_rules_page
        FOREIGN KEY (organization_id, status_page_id)
        REFERENCES status_pages(org_id, id) ON DELETE CASCADE,
    CONSTRAINT chk_status_page_automation_rules_position CHECK (position >= 0),
    CONSTRAINT chk_status_page_automation_rules_lifecycle
        CHECK (lifecycle IN ('draft', 'active', 'paused', 'archived'))
);
CREATE INDEX ix_status_page_automation_rules_order
    ON status_page_automation_rules (organization_id, status_page_id, position, id)
    WHERE lifecycle <> 'archived';

CREATE TABLE status_page_automation_revisions (
    id                      VARCHAR(64) PRIMARY KEY,
    organization_id         VARCHAR(64) NOT NULL,
    status_page_id          VARCHAR(64) NOT NULL,
    rule_id                 VARCHAR(64) NOT NULL,
    number                  INTEGER NOT NULL,
    matchers                JSONB NOT NULL,
    action                  JSONB NOT NULL,
    content_hash            VARCHAR(64) NOT NULL,
    created_by              VARCHAR(64) NOT NULL REFERENCES users(id),
    created_at_micros       BIGINT NOT NULL,
    CONSTRAINT uq_status_page_automation_revisions_org_id UNIQUE (organization_id, id),
    CONSTRAINT uq_status_page_automation_revisions_number UNIQUE (rule_id, number),
    CONSTRAINT fk_status_page_automation_revisions_rule
        FOREIGN KEY (organization_id, rule_id)
        REFERENCES status_page_automation_rules(organization_id, id) ON DELETE CASCADE,
    CONSTRAINT fk_status_page_automation_revisions_page
        FOREIGN KEY (organization_id, status_page_id)
        REFERENCES status_pages(org_id, id) ON DELETE CASCADE,
    CONSTRAINT chk_status_page_automation_revisions_number CHECK (number > 0)
);
ALTER TABLE status_page_automation_rules
    ADD CONSTRAINT fk_status_page_automation_rules_active_revision
        FOREIGN KEY (organization_id, active_revision_id)
        REFERENCES status_page_automation_revisions(organization_id, id)
        DEFERRABLE INITIALLY DEFERRED,
    ADD CONSTRAINT fk_status_page_automation_rules_draft_revision
        FOREIGN KEY (organization_id, draft_revision_id)
        REFERENCES status_page_automation_revisions(organization_id, id)
        DEFERRABLE INITIALLY DEFERRED;

CREATE TABLE status_page_automation_candidates (
    id                      VARCHAR(64) PRIMARY KEY,
    organization_id         VARCHAR(64) NOT NULL,
    status_page_id          VARCHAR(64) NOT NULL,
    rule_revision_id        VARCHAR(64) NOT NULL,
    correlation_key         VARCHAR(255) NOT NULL,
    state                   VARCHAR(24) NOT NULL,
    title                   VARCHAR(255) NOT NULL,
    message                 TEXT NOT NULL,
    resolved_message        TEXT NOT NULL,
    impact                  VARCHAR(16) NOT NULL,
    component_ids           TEXT[] NOT NULL,
    automatic               BOOLEAN NOT NULL,
    status_incident_id      VARCHAR(64),
    due_at_micros           BIGINT NOT NULL,
    last_error              TEXT,
    created_at_micros       BIGINT NOT NULL,
    updated_at_micros       BIGINT NOT NULL,
    CONSTRAINT uq_status_page_automation_candidates_org_id UNIQUE (organization_id, id),
    CONSTRAINT fk_status_page_automation_candidates_page
        FOREIGN KEY (organization_id, status_page_id)
        REFERENCES status_pages(org_id, id) ON DELETE CASCADE,
    CONSTRAINT fk_status_page_automation_candidates_revision
        FOREIGN KEY (organization_id, rule_revision_id)
        REFERENCES status_page_automation_revisions(organization_id, id),
    CONSTRAINT fk_status_page_automation_candidates_incident
        FOREIGN KEY (organization_id, status_page_id, status_incident_id)
        REFERENCES status_page_incidents(org_id, status_page_id, id),
    CONSTRAINT chk_status_page_automation_candidates_state
        CHECK (state IN ('delayed', 'pending_approval', 'approved', 'published', 'rejected',
                         'cancelled', 'resolved', 'failed')),
    CONSTRAINT chk_status_page_automation_candidates_impact
        CHECK (impact IN ('minor', 'major', 'critical')),
    CONSTRAINT chk_status_page_automation_candidates_components
        CHECK (CARDINALITY(component_ids) BETWEEN 1 AND 500)
);
CREATE UNIQUE INDEX uq_status_page_automation_candidates_open
    ON status_page_automation_candidates
       (organization_id, status_page_id, correlation_key)
    WHERE state IN ('delayed', 'pending_approval', 'approved', 'published', 'failed');
CREATE INDEX ix_status_page_automation_candidates_due
    ON status_page_automation_candidates (due_at_micros, created_at_micros)
    WHERE state = 'delayed';

CREATE TABLE status_page_automation_candidate_sources (
    organization_id         VARCHAR(64) NOT NULL,
    candidate_id            VARCHAR(64) NOT NULL,
    source_kind             VARCHAR(32) NOT NULL,
    source_id               VARCHAR(64) NOT NULL,
    source_instance_id      VARCHAR(64) NOT NULL,
    severity                VARCHAR(16) NOT NULL,
    labels                  JSONB NOT NULL DEFAULT '{}'::JSONB,
    active                  BOOLEAN NOT NULL,
    muted                   BOOLEAN NOT NULL,
    observed_at_micros      BIGINT NOT NULL,
    updated_at_micros       BIGINT NOT NULL,
    PRIMARY KEY (organization_id, candidate_id, source_kind, source_instance_id),
    CONSTRAINT fk_status_page_automation_candidate_sources_candidate
        FOREIGN KEY (organization_id, candidate_id)
        REFERENCES status_page_automation_candidates(organization_id, id) ON DELETE CASCADE,
    CONSTRAINT chk_status_page_automation_candidate_sources_kind
        CHECK (source_kind IN ('alert_incident', 'synthetic_monitor')),
    CONSTRAINT chk_status_page_automation_candidate_sources_severity
        CHECK (severity IN ('info', 'warning', 'error', 'critical'))
);

CREATE TABLE status_page_automation_actions (
    id                      VARCHAR(64) PRIMARY KEY,
    organization_id         VARCHAR(64) NOT NULL,
    candidate_id            VARCHAR(64) NOT NULL,
    action                  VARCHAR(32) NOT NULL,
    actor_id                VARCHAR(64) REFERENCES users(id),
    note                    TEXT,
    created_at_micros       BIGINT NOT NULL,
    CONSTRAINT fk_status_page_automation_actions_candidate
        FOREIGN KEY (organization_id, candidate_id)
        REFERENCES status_page_automation_candidates(organization_id, id) ON DELETE CASCADE,
    CONSTRAINT chk_status_page_automation_actions_action
        CHECK (action IN ('created', 'pending_approval', 'approved', 'published', 'rejected',
                          'cancelled', 'resolved', 'failed', 'retried'))
);

CREATE TABLE status_page_automation_outbox (
    id                      VARCHAR(64) PRIMARY KEY,
    organization_id         VARCHAR(64) NOT NULL,
    candidate_id            VARCHAR(64) NOT NULL,
    kind                    VARCHAR(32) NOT NULL,
    idempotency_key         VARCHAR(255) NOT NULL UNIQUE,
    payload                 JSONB NOT NULL,
    status                  VARCHAR(16) NOT NULL DEFAULT 'pending',
    attempts                INTEGER NOT NULL DEFAULT 0,
    available_at_micros     BIGINT NOT NULL,
    locked_at_micros        BIGINT,
    last_error              TEXT,
    completed_at_micros     BIGINT,
    created_at_micros       BIGINT NOT NULL,
    updated_at_micros       BIGINT NOT NULL,
    CONSTRAINT fk_status_page_automation_outbox_candidate
        FOREIGN KEY (organization_id, candidate_id)
        REFERENCES status_page_automation_candidates(organization_id, id) ON DELETE CASCADE,
    CONSTRAINT chk_status_page_automation_outbox_kind
        CHECK (kind IN ('publish', 'resolve', 'notify_publisher')),
    CONSTRAINT chk_status_page_automation_outbox_status
        CHECK (status IN ('pending', 'processing', 'completed', 'dead_letter')),
    CONSTRAINT chk_status_page_automation_outbox_attempts CHECK (attempts >= 0)
);
CREATE INDEX ix_status_page_automation_outbox_claim
    ON status_page_automation_outbox (available_at_micros, created_at_micros)
    WHERE status IN ('pending', 'processing');

CREATE TABLE status_page_automation_settings (
    organization_id         VARCHAR(64) NOT NULL,
    status_page_id          VARCHAR(64) NOT NULL,
    paused                  BOOLEAN NOT NULL DEFAULT FALSE,
    updated_by              VARCHAR(64) NOT NULL REFERENCES users(id),
    updated_at_micros       BIGINT NOT NULL,
    PRIMARY KEY (organization_id, status_page_id),
    CONSTRAINT fk_status_page_automation_settings_page
        FOREIGN KEY (organization_id, status_page_id)
        REFERENCES status_pages(org_id, id) ON DELETE CASCADE
);

-- ============================================================
-- Synthetic Probe Agent organization configuration
-- ============================================================

-- SPDX-License-Identifier: Apache-2.0
-- Copyright (c) 2026 MoleSignal Authors

-- Agent heartbeats own runtime state. Organizations own only stable display
-- metadata, including an organization-specific view of shared platform Agents.
CREATE TABLE synthetic_probe_agent_configurations (
    organization_id         VARCHAR(64) NOT NULL
                                REFERENCES organizations(id) ON DELETE CASCADE,
    agent_id                VARCHAR(64) NOT NULL
                                REFERENCES synthetic_probe_agents(id) ON DELETE CASCADE,
    name                    VARCHAR(255) NOT NULL,
    labels                  JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_by              VARCHAR(64) REFERENCES users(id) ON DELETE SET NULL,
    updated_by              VARCHAR(64) REFERENCES users(id) ON DELETE SET NULL,
    created_at_micros       BIGINT NOT NULL,
    updated_at_micros       BIGINT NOT NULL,
    PRIMARY KEY (organization_id, agent_id),
    CONSTRAINT chk_synthetic_probe_agent_configurations_name
        CHECK (CHAR_LENGTH(BTRIM(name)) BETWEEN 1 AND 255),
    CONSTRAINT chk_synthetic_probe_agent_configurations_labels
        CHECK (JSONB_TYPEOF(labels) = 'object')
);

-- ============================================================
-- Synthetic result pagination
-- ============================================================

-- Global result pages are ordered within one organization, independently of monitor.
CREATE INDEX ix_synthetic_results_org_time
    ON synthetic_results (organization_id, finished_at_micros DESC, id DESC);
