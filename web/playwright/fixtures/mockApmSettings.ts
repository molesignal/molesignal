/**
 * The backend of the APM settings page, beside the APM one: the
 * organization's settings, the platform's cardinality limits and its trace
 * sampling policy, each kept as saved; and an overview of the day whose
 * collection quality has something to say: endpoints past their limit, an
 * environment written two ways and a service named as its pod.
 */
import type { Page } from '@playwright/test';

import { APM_SETTINGS, overview, SERVICE } from './mockApm';

const LIMITS = {
  servicesPerOrgHour: 200,
  transactionsPerServiceHour: 32,
  dependenciesPerServiceHour: 16,
  operationsPerDependencyHour: 32,
  errorGroupsPerServiceHour: 16,
  versionsPerServiceHour: 16,
  instancesPerServiceHour: 256,
};

const POLICY = {
  version: 3,
  enabled: true,
  normalSampleRatio: 0.1,
  decisionWindowMs: 30_000,
  rootGraceMs: 1_000,
  decisionCacheMs: 300_000,
  maxTraces: 10_000,
  memoryBytes: 268_435_456,
  maxSpansPerTrace: 1_000,
  slowThresholdsMs: { http: 1_000, query: 5_000, batchIntake: 2_000, database: 500, objectStore: 2_000, external: 2_000, background: 10_000, other: 1_000 },
  routeSlowThresholdsMs: {},
  rules: [{ name: 'Keep checkout errors', serviceName: 'checkout', route: null, operation: null, minimumDurationMs: null, requireError: true, action: { type: 'keep' } }],
};

export interface SettingsCaptured {
  /** What each save sent, in order. */
  settings: Array<Record<string, unknown>>;
  limits: Array<Record<string, unknown>>;
  policies: Array<Record<string, unknown>>;
}

export async function installApmSettingsRoutes(page: Page): Promise<SettingsCaptured> {
  const captured: SettingsCaptured = { settings: [], limits: [], policies: [] };
  let settings: Record<string, unknown> = { ...APM_SETTINGS };
  let limits: Record<string, unknown> = { ...LIMITS };
  let policy: Record<string, unknown> = { ...POLICY };

  await page.route(/\/api\/v1\/apm\/overview(?:\?.*)?$/, (route) =>
    route.fulfill({
      json: {
        ...overview,
        meta: { ...overview.meta, dataQuality: { ...overview.meta.dataQuality, overflowDimensions: ['transaction'] } },
        services: [...overview.services, { ...overview.services[0], service: { ...SERVICE, name: 'checkout-7d9f8b6c5-x2x7k' } }],
        availableEnvironments: ['prod', 'Production', 'production'],
      },
    }),
  );
  await page.route(/\/api\/v1\/apm\/settings$/, async (route) => {
    if (route.request().method() === 'PUT') {
      settings = route.request().postDataJSON() as Record<string, unknown>;
      captured.settings.push(settings);
    }
    await route.fulfill({ json: settings });
  });
  await page.route(/\/api\/v1\/settings\/apm_limits$/, async (route) => {
    if (route.request().method() === 'PUT') {
      limits = route.request().postDataJSON() as Record<string, unknown>;
      captured.limits.push(limits);
    }
    await route.fulfill({ json: limits });
  });
  await page.route(/\/api\/v1\/system\/telemetry$/, async (route) => {
    if (route.request().method() === 'PUT') {
      const sent = route.request().postDataJSON() as Record<string, unknown>;
      captured.policies.push(sent);
      policy = { ...sent, version: Number(policy.version) + 1 };
    }
    await route.fulfill({
      json: { deploymentForceDisabled: false, runtimeEnabled: true, effectiveEnabled: true, policy, health: { status: 'ok', detail: 'tail sampler and configured sinks are healthy' } },
    });
  });

  return captured;
}
