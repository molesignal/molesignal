// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

/**
 * Deterministic APM capacity model used by the OpenSpec capacity gate.
 *
 * This is deliberately not a database benchmark. It turns the accepted
 * cardinality envelope into worst/working-set row and byte budgets so the
 * later PostgreSQL load test has explicit pass/fail thresholds.
 */

const target = Object.freeze({
  sustainedSpansPerSecondPerOwner: 20_000,
  burstSpansPerSecondPerOwner: 50_000,
  burstSeconds: 30,
  projectorOwners: 4,
  servicesPerOrganizationHour: 200,
  transactionsPerServiceHour: 32,
  dependenciesPerServiceHour: 16,
  errorGroupsPerServiceHour: 16,
  versionsPerServiceHour: 16,
  instancesPerServiceHour: 256,
  activeDimensionRatioPerMinute: 0.05,
  hotRetentionHours: 24,
  rollupRetentionDays: 30,
  // Rounded up from the PostgreSQL 17 release benchmark, including indexes.
  estimatedPostgresBytesPerAggregateRow: Object.freeze({
    service: 1_030,
    transaction: 1_110,
    dependency: 1_110,
    error: 1_220,
  }),
});

const minutesPerHour = 60;
const hoursPerDay = 24;

const serviceRowsPerMinute =
  target.servicesPerOrganizationHour * target.projectorOwners;
const transactionRowsPerMinute =
  target.servicesPerOrganizationHour *
  target.transactionsPerServiceHour *
  target.activeDimensionRatioPerMinute *
  target.projectorOwners;
const dependencyRowsPerMinute =
  target.servicesPerOrganizationHour *
  target.dependenciesPerServiceHour *
  target.activeDimensionRatioPerMinute *
  target.projectorOwners;
const errorRowsPerMinute =
  target.servicesPerOrganizationHour *
  target.errorGroupsPerServiceHour *
  target.activeDimensionRatioPerMinute *
  target.projectorOwners;
const hotRowsPerMinute =
  serviceRowsPerMinute +
  transactionRowsPerMinute +
  dependencyRowsPerMinute +
  errorRowsPerMinute;
const hotRows =
  hotRowsPerMinute * minutesPerHour * target.hotRetentionHours;

// Hourly rollups collapse owner_id. The gate deliberately assumes every
// allowed dimension is observed during each hour.
const serviceRollupRowsPerHour = target.servicesPerOrganizationHour;
const transactionRollupRowsPerHour =
  target.servicesPerOrganizationHour *
  target.transactionsPerServiceHour;
const dependencyRollupRowsPerHour =
  target.servicesPerOrganizationHour *
  target.dependenciesPerServiceHour;
const errorRollupRowsPerHour =
  target.servicesPerOrganizationHour *
  target.errorGroupsPerServiceHour;
const rollupRowsPerHour =
  serviceRollupRowsPerHour +
  transactionRollupRowsPerHour +
  dependencyRollupRowsPerHour +
  errorRollupRowsPerHour;
const rollupRows =
  rollupRowsPerHour * hoursPerDay * target.rollupRetentionDays;

const hotBytes =
  serviceRowsPerMinute *
    minutesPerHour *
    target.hotRetentionHours *
    target.estimatedPostgresBytesPerAggregateRow.service +
  transactionRowsPerMinute *
    minutesPerHour *
    target.hotRetentionHours *
    target.estimatedPostgresBytesPerAggregateRow.transaction +
  dependencyRowsPerMinute *
    minutesPerHour *
    target.hotRetentionHours *
    target.estimatedPostgresBytesPerAggregateRow.dependency +
  errorRowsPerMinute *
    minutesPerHour *
    target.hotRetentionHours *
    target.estimatedPostgresBytesPerAggregateRow.error;
const rollupBytes =
  serviceRollupRowsPerHour *
    hoursPerDay *
    target.rollupRetentionDays *
    target.estimatedPostgresBytesPerAggregateRow.service +
  transactionRollupRowsPerHour *
    hoursPerDay *
    target.rollupRetentionDays *
    target.estimatedPostgresBytesPerAggregateRow.transaction +
  dependencyRollupRowsPerHour *
    hoursPerDay *
    target.rollupRetentionDays *
    target.estimatedPostgresBytesPerAggregateRow.dependency +
  errorRollupRowsPerHour *
    hoursPerDay *
    target.rollupRetentionDays *
    target.estimatedPostgresBytesPerAggregateRow.error;

const gib = (bytes) => Number((bytes / 1024 ** 3).toFixed(2));

const output = {
  target,
  workingSet: {
    serviceRowsPerMinute,
    transactionRowsPerMinute,
    dependencyRowsPerMinute,
    errorRowsPerMinute,
    hotRowsPerMinute,
    hotRows,
    serviceRollupRowsPerHour,
    transactionRollupRowsPerHour,
    dependencyRollupRowsPerHour,
    errorRollupRowsPerHour,
    rollupRowsPerHour,
    rollupRows,
    estimatedHotGiBPerHeavyOrganization: gib(hotBytes),
    estimatedRollupGiBPerHeavyOrganization: gib(rollupBytes),
    estimatedTotalGiBPerHeavyOrganization: gib(hotBytes + rollupBytes),
  },
  gates: {
    maxHotRowsPerHeavyOrganization: 11_000_000,
    maxEstimatedTotalGiBPerHeavyOrganization: 16,
  },
};

if (
  output.workingSet.hotRows >
    output.gates.maxHotRowsPerHeavyOrganization ||
  output.workingSet.estimatedTotalGiBPerHeavyOrganization >
    output.gates.maxEstimatedTotalGiBPerHeavyOrganization
) {
  console.error(JSON.stringify(output, null, 2));
  process.exitCode = 1;
} else {
  console.log(JSON.stringify(output, null, 2));
}
