-- SPDX-License-Identifier: Apache-2.0
-- Copyright (c) 2026 MoleSignal Authors

-- Status Page management is a first-class collaboration area. Normalize the
-- route catalog while the previous navigation constraint is absent, then
-- install the final closed set of navigation groups.
ALTER TABLE iam_routes DROP CONSTRAINT chk_iam_routes_navigation;
UPDATE iam_routes SET navigation_group = 'data' WHERE navigation_group = 'pipeline';
UPDATE iam_routes
SET navigation_group = 'collaboration', navigation_position = 10
WHERE route_key = 'reports';
ALTER TABLE iam_routes
    ADD CONSTRAINT chk_iam_routes_navigation
    CHECK (
        (navigation_group IS NULL AND navigation_position IS NULL)
        OR (
            navigation_group IN ('home', 'investigate', 'data', 'collaboration', 'admin')
            AND navigation_position >= 0
        )
    );

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

INSERT INTO iam_routes (
    route_key, path_pattern, scope, permission_mode, required_features,
    navigation_group, navigation_position, enabled, catalog_version
)
VALUES (
    'status.pages', '/status-pages', 'organization', 'all', ARRAY[]::TEXT[],
    'collaboration', 20, TRUE, 4
)
ON CONFLICT (route_key) DO UPDATE
SET path_pattern = EXCLUDED.path_pattern,
    scope = EXCLUDED.scope,
    permission_mode = EXCLUDED.permission_mode,
    required_features = EXCLUDED.required_features,
    navigation_group = EXCLUDED.navigation_group,
    navigation_position = EXCLUDED.navigation_position,
    enabled = EXCLUDED.enabled,
    catalog_version = EXCLUDED.catalog_version;
DELETE FROM iam_route_permissions WHERE route_key = 'status.pages';
INSERT INTO iam_route_permissions (route_key, permission_key, position)
VALUES ('status.pages', 'status_pages.read', 0);

UPDATE iam_route_catalog_versions
SET version = GREATEST(version, 4),
    updated_at_micros = (EXTRACT(EPOCH FROM clock_timestamp()) * 1000000)::BIGINT
WHERE catalog_key = 'routes';

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
