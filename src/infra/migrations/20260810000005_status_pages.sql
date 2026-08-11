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
