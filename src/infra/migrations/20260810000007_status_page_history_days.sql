-- SPDX-License-Identifier: Apache-2.0
-- Copyright (c) 2026 MoleSignal Authors

-- The configured window controls customer-visible resolved incident history.
-- Uptime calculations may still read at least 90 days of incident data.
ALTER TABLE status_pages
    ADD COLUMN history_days INTEGER NOT NULL DEFAULT 90;

ALTER TABLE status_pages
    ADD CONSTRAINT chk_status_pages_history_days
    CHECK (history_days BETWEEN 1 AND 365);
