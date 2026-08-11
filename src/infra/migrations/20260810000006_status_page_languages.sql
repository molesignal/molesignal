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
