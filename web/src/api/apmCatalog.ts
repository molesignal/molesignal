import { http } from '@/lib/http';

import type { TransactionIdentity } from './apm';

/** How much a service matters, from P0, the most, to P5. */
export type ServiceTier = 'p0' | 'p1' | 'p2' | 'p3' | 'p4' | 'p5';
export type ServiceLifecycle = 'production' | 'staging' | 'experimental' | 'deprecated';

/** A log stream that holds the service's logs, with the condition that picks them out of it. */
export interface ServiceLogSource {
  stream: string;
  /** A boolean SQL expression; none when the stream holds this service's logs alone. */
  filter?: string | null;
}

/** The service's metrics as a PromQL selector: `{service="order-api"}` or a metric with its labels. */
export interface ServiceMetricSource {
  selector: string;
}

/** A page about the service elsewhere: a runbook, a document, a CMDB entry. */
export interface ServiceLink {
  name: string;
  url: string;
}

/** Where a service's data is. Older backends answer without it. */
export interface ServiceDataSources {
  logs?: ServiceLogSource[];
  metrics?: ServiceMetricSource[];
  /** Ids of the dashboards that show the service. */
  dashboards?: string[];
  links?: ServiceLink[];
}

/** A service as people describe it, in every environment it runs in. Owners are the platform's teams and users. */
export interface ServiceDefinition {
  namespace: string;
  name: string;
  description: string;
  teamId?: string | null;
  ownerId?: string | null;
  tier?: ServiceTier | null;
  lifecycle?: ServiceLifecycle | null;
  /** The code's repository, for a service whose telemetry does not say it. */
  repositoryUrl?: string | null;
  /** `key` → `value`; an empty value is the key alone. Older backends answer without it. */
  tags?: Record<string, string>;
  /** The Apdex threshold T of the service's requests, in place of the default. */
  apdexThreshold?: number | null;
  /** The endpoints that matter most, starred in the service's endpoint lists. */
  keyTransactions?: TransactionIdentity[];
  /** Other names the service goes by in logs, metrics and elsewhere. Older backends answer without them. */
  aliases?: string[];
  dataSources?: ServiceDataSources;
  /** Out of the catalog's default view. */
  archived?: boolean;
  updatedBy: string;
  updatedAt: number;
}

/** What a save writes; key transactions are starred one at a time, and a save keeps them. */
export type ServiceDefinitionWrite = Pick<
  ServiceDefinition,
  'description' | 'teamId' | 'ownerId' | 'tier' | 'lifecycle' | 'repositoryUrl' | 'tags' | 'apdexThreshold' | 'aliases' | 'dataSources' | 'archived'
>;

/** Whether a signal carries the service: `unknown` when the look failed or could not tell. */
export type CoverageState = 'reported' | 'missing' | 'unknown';

export interface SignalCoverage {
  state: CoverageState;
  /** Where it was found: log streams, profile types, a RUM session. */
  found?: string[];
}

/** The signals a service reports beside its traces, over the day before `to`. */
export interface ServiceCoverage {
  from: number;
  to: number;
  logs: SignalCoverage;
  metrics: SignalCoverage;
  profiles: SignalCoverage;
  rum: SignalCoverage;
}

export const apmCatalogApi = {
  /** Stars an endpoint as a key transaction of its service, or takes the star off. */
  markKeyTransaction: async (service: { namespace: string; name: string }, transaction: TransactionIdentity, key: boolean) => {
    const { data } = await http.post<ServiceDefinition>(
      `/apm/service-definitions/${encodeURIComponent(service.name)}/key-transactions`,
      { transaction, key },
      { params: { namespace: service.namespace } },
    );
    return data;
  },
  coverage: async (service: { namespace: string; name: string; environment: string }, to: number) => {
    const { data } = await http.get<ServiceCoverage>(`/apm/services/${encodeURIComponent(service.name)}/coverage`, {
      params: { namespace: service.namespace, environment: service.environment, to },
    });
    return data;
  },
};

export const apmCatalogQueryKeys = {
  coverage: (orgId: string, service: { namespace: string; name: string; environment: string }, slot: number) =>
    ['apm', 'coverage', orgId, service.namespace, service.name, service.environment, slot] as const,
};
