import { http } from '@/lib/http';

import type { ServiceTier } from './apmCatalog';

/** From what error rate and P95 a service is a warning, and critical. */
export interface HealthLines {
  errorRateWarning: number;
  errorRateCritical: number;
  p95WarningTime: number;
  p95CriticalTime: number;
}

/** A service name matching `pattern` (a regular expression) becomes `replacement`, which may name its groups. */
export interface NamingRule {
  pattern: string;
  replacement: string;
}

/** An environment named `from` is read as `to`. */
export interface EnvironmentMapping {
  from: string;
  to: string;
}

/** What `alias_namespace/alias` reported is read as `namespace/service`'s. */
export interface ServiceAlias {
  aliasNamespace: string;
  alias: string;
  namespace: string;
  service: string;
}

/** What the catalog asks of a service from a tier on; `null` asks nothing. */
export interface MetadataRules {
  teamFromTier: ServiceTier | null;
  ownerFromTier: ServiceTier | null;
  repositoryFromTier: ServiceTier | null;
}

/** What an organization sets for APM. */
export interface ApmSettings {
  health: HealthLines;
  /** The Apdex threshold T of a service whose definition sets none. */
  apdexThreshold: number;
  /** From what P99 an endpoint is a long task. */
  longTaskTime: number;
  namingRules: NamingRule[];
  environmentMap: EnvironmentMapping[];
  aliases: ServiceAlias[];
  metadataRules: MetadataRules;
}

/** What the server reads when an organization has set nothing. */
export const DEFAULT_APM_SETTINGS: ApmSettings = {
  health: {
    errorRateWarning: 0.02,
    errorRateCritical: 0.1,
    p95WarningTime: 500_000,
    p95CriticalTime: 2_000_000,
  },
  apdexThreshold: 500_000,
  longTaskTime: 1_000_000,
  namingRules: [],
  environmentMap: [],
  aliases: [],
  metadataRules: { teamFromTier: null, ownerFromTier: null, repositoryFromTier: null },
};

/** The most of each kind APM keeps apart an hour; past them the rest are counted as overflow. */
export interface ApmLimits {
  servicesPerOrgHour: number;
  transactionsPerServiceHour: number;
  dependenciesPerServiceHour: number;
  operationsPerDependencyHour: number;
  errorGroupsPerServiceHour: number;
  versionsPerServiceHour: number;
  instancesPerServiceHour: number;
}

export const APM_LIMIT_MAX = 100_000;

export const apmSettingsApi = {
  get: async () => {
    const { data } = await http.get<ApmSettings>('/apm/settings');
    return data;
  },
  save: async (settings: ApmSettings) => {
    const { data } = await http.put<ApmSettings>('/apm/settings', settings);
    return data;
  },
  /** The platform's limits (`sys.settings.limits.manage`). */
  limits: async () => {
    const { data } = await http.get<ApmLimits>('/settings/apm_limits');
    return data;
  },
  saveLimits: async (limits: ApmLimits) => {
    const { data } = await http.put<ApmLimits>('/settings/apm_limits', limits);
    return data;
  },
};

export const apmSettingsQueryKeys = {
  settings: (orgId: string) => ['apm', 'settings', orgId] as const,
  limits: () => ['apm', 'limits'] as const,
};
