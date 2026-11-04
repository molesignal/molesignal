import { http } from '@/lib/http';
import type { Severity } from '@/types/alerting';

/** What automatic discovery finds without a rule. */
export type FindingKind =
  | 'apm_anomaly'
  | 'apm_regression'
  | 'rum_new_issue'
  | 'rum_regressed_issue'
  | 'log_error_surge'
  | 'log_new_error'
  | 'metric_saturation'
  | 'metric_shift'
  | 'metric_restarts';

export const FINDING_KINDS: readonly FindingKind[] = [
  'apm_anomaly',
  'apm_regression',
  'rum_new_issue',
  'rum_regressed_issue',
  'log_error_surge',
  'log_new_error',
  'metric_saturation',
  'metric_shift',
  'metric_restarts',
];

/** Which side of the product a finding comes from. */
export type FindingSource = 'apm' | 'rum' | 'logs' | 'metrics';

export function sourceOf(kind: FindingKind): FindingSource {
  if (kind.startsWith('rum_')) return 'rum';
  if (kind.startsWith('log_')) return 'logs';
  if (kind.startsWith('metric_')) return 'metrics';
  return 'apm';
}

/** A nearby change of an APM finding: a deployment or a version first seen. */
export interface FindingChange {
  kind: string;
  source: string;
  service: string;
  version: string;
  status: string;
  at: number;
  downstream?: boolean;
  commit?: string | null;
  description?: string | null;
}

export interface Finding {
  id: string;
  orgId: string;
  kind: FindingKind;
  key: string;
  severity: Severity;
  /** `namespace/name` of an APM service, or the id of a RUM application. */
  subject: string;
  environment?: string | null;
  /** What moved, for an APM anomaly: `error_rate`, `p95` (micros) or `throughput` (per second). */
  metric?: string | null;
  title: string;
  value?: number | null;
  /** What it is compared with: the stretch before, or the version before. */
  baseline?: number | null;
  startedAt: number;
  lastSeenAt: number;
  /** Set once it went unseen for three rounds. */
  endedAt?: number | null;
  missedRounds: number;
  detail: {
    previous?: number | null;
    lastWeek?: number | null;
    version?: string | null;
    previousVersion?: string | null;
    changes?: FindingChange[];
    issueId?: string;
    application?: string;
    errorType?: string;
    location?: string;
    /** Log findings: the stream, and the error template of a new error. */
    stream?: string;
    template?: string;
    /** Metric findings: the PromQL that found it, and the labels of the series. */
    promql?: string;
    labels?: Record<string, string>;
  };
  createdAt: number;
  updatedAt: number;
}

export type FindingStatus = 'open' | 'ended' | 'all';

/** The most recently seen first. */
export async function listFindings(options: {
  status?: FindingStatus;
  kind?: FindingKind;
  limit?: number;
} = {}): Promise<Finding[]> {
  const { data } = await http.get<Finding[]>('/alerts/discovery/findings', { params: options });
  return data;
}

export async function getFinding(id: string): Promise<Finding> {
  const { data } = await http.get<Finding>(`/alerts/discovery/findings/${encodeURIComponent(id)}`);
  return data;
}
