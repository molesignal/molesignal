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
