# Cloud Marketplace Capability ()

## Purpose

AWS / Azure Marketplace 订阅生命周期回调与 metering API 上报，用于 SaaS 部署计费。 特性。

## Requirements

### Requirement: Marketplace subscription lifecycle

The system SHALL expose AWS Marketplace and Azure Marketplace webhook endpoints (`POST /api/v1/_marketplace/aws/notify` / `/azure/notify`) and verify their signatures. Subscription state transitions `pending → active → suspended → cancelled` SHALL persist to `marketplace_subscriptions` table.

#### Scenario: AWS subscribe-success notification activates org

- **WHEN** AWS SNS posts a `subscribe-success` notification with valid signature
- **THEN** the corresponding `marketplace_subscriptions` row state becomes `active` and the linked org's `[license.entitled_features]` is updated

### Requirement: Metering API

The system SHALL aggregate per-org metered usage (ingest bytes, query count, active users) and report hourly via AWS `MeterUsage` / Azure `usageEvent` APIs. Failures SHALL retry with exponential backoff up to 24h.

#### Scenario: MeterUsage call

- **WHEN** the hourly aggregator runs for org X with `ingest_bytes=1.2GiB`
- **THEN** the system calls `aws-marketplace-metering:MeterUsage` with dimension `ingest_gib` and quantity `2` (ceiling to integer)

### Requirement: License gating

Marketplace endpoints SHALL be compiled only when `feature = ""`. OSS build returns 404 at the route.

#### Scenario: OSS returns 404

- **WHEN** OSS build and AWS SNS posts to `/api/v1/_marketplace/aws/notify`
- **THEN** the system returns 404 (route not registered)
