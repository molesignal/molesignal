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
