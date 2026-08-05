# Log Patterns Capability

## Purpose

正则模式 CRUD + 命中分类 + 模式提取归类。`vectorscan` 作为 optional feature 用于加速。

## Requirements

### Requirement: Pattern CRUD

The system SHALL expose `/api/v1/log_patterns` to create / list / update / delete regex-based log patterns. Each pattern carries `{ id, org_id, name, regex, capture_groups[], category, priority, stream_filter? }`. Patterns SHALL be compiled with `regex::Regex` on create and rejected with 400 if compilation fails.

#### Scenario: Bad regex rejected on create

- **WHEN** a user POSTs `{ "name": "bad", "regex": "[invalid(" }`
- **THEN** the system returns 400 with body `{ "error": "regex parse error: ..." }`

### Requirement: Pattern application at query time

The system SHALL provide a SQL function `extract_pattern(message_column)` that returns the matched pattern category (or NULL if no match). Patterns SHALL be evaluated in declining `priority` order; the first match wins.

#### Scenario: Pattern extraction in SELECT

- **WHEN** a user runs `SELECT extract_pattern(message) AS cat, count(*) FROM logs GROUP BY cat`
- **THEN** rows where `message` matches a known pattern carry that pattern's category in `cat`

### Requirement: Optional vectorscan acceleration

When the binary is built with `feature = "vectorscan"`, the system SHALL use HyperScan (Intel vectorscan fork) for multi-pattern matching at >10x throughput. Without the feature, falls back to sequential `regex::Regex` evaluation.

#### Scenario: Vectorscan feature off

- **WHEN** the binary built without `vectorscan`
- **THEN** `extract_pattern` still produces correct results, just slower
