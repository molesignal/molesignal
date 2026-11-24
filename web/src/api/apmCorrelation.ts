import { http } from '@/lib/http';

import type { ApmQueryParams, ServiceIdentity } from './apm';
import type { CoverageState } from './apmCatalog';

/** How well a service ties to other data one way: `unknown` when a look failed. */
export type CorrelationGrade = 'good' | 'partial' | 'missing' | 'unknown';

/** What the logs say of one service over the look window. */
export interface LogCorrelation {
  state: CoverageState;
  streams: string[];
  lines: number;
  /** Lines carrying a trace id. */
  traced: number;
  /** Trace ids taken from recent lines to look for among the traces, and how many were found. */
  sampled: number;
  matched: number;
}

/** What a service's spans say about it. */
export interface ResourceCompleteness {
  environment: boolean;
  version: boolean;
  instance: boolean;
  /** An instance said where it runs: its host, pod, node or container. */
  placement: boolean;
}

export interface CorrelationGrades {
  traceIds: CorrelationGrade;
  resource: CorrelationGrade;
  metrics: CorrelationGrade;
  changes: CorrelationGrade;
}

export interface ServiceCorrelation {
  service: ServiceIdentity;
  logs: LogCorrelation;
  metrics: CoverageState;
  resource: ResourceCompleteness;
  /** Changes reported since `changesSince`. */
  changes: { count: number; lastAt?: number };
  grades: CorrelationGrades;
}

/** A service name in logs or metrics no APM service answers to, naming rules and aliases applied. */
export interface UnmatchedName {
  name: string;
  signal: 'logs' | 'metrics';
  streams: string[];
  /** Log lines under the name. */
  lines?: number;
}

export interface CorrelationReport {
  /** The window logs and metrics were looked at over. */
  from: number;
  to: number;
  changesSince: number;
  services: ServiceCorrelation[];
  unmatched: UnmatchedName[];
}

export const apmCorrelationApi = {
  report: async (params: ApmQueryParams) => {
    const { data } = await http.get<CorrelationReport>('/apm/correlation', { params });
    return data;
  },
};

/** The backend keeps a report five minutes: a range moving within them asks for the same one. */
const REPORT_SLOT_MICROS = 300_000_000;

const slot = (micros: number | undefined) => (micros === undefined ? null : Math.floor(micros / REPORT_SLOT_MICROS));

export const apmCorrelationQueryKeys = {
  report: (orgId: string, params: ApmQueryParams) =>
    ['apm', 'correlation', orgId, params.namespace ?? null, params.environment ?? null, slot(params.from), slot(params.to)] as const,
};
