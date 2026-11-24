import { http } from '@/lib/http';

/** How a reported change went. */
export type DeploymentStatus = 'succeeded' | 'failed' | 'rolled_back';

/** What pipelines and tools report about a service. */
export type ReportedChangeKind = 'deployment' | 'configuration' | 'feature_flag' | 'scaling' | 'infrastructure';

/** Every kind a change list tells: reported, annotations, versions APM first saw, alert rule edits. */
export type ChangeKind = ReportedChangeKind | 'annotation' | 'version' | 'rule';

export const REPORTED_CHANGE_KINDS: readonly ReportedChangeKind[] = ['deployment', 'configuration', 'feature_flag', 'scaling', 'infrastructure'];

/** One change; each part is said only when known. */
export interface ChangeEntry {
  id: string;
  kind: ChangeKind;
  at: number;
  /** When a change over a range ended: an annotation's end. */
  end?: number;
  service?: string;
  namespace?: string;
  environment?: string;
  version?: string;
  status?: DeploymentStatus;
  /** An annotation's or a rule's title. */
  title?: string;
  description?: string;
  tags?: string[];
  commit?: string;
  /** The code's repository, its credentials masked. */
  repositoryUrl?: string;
  pullRequestUrl?: string;
  author?: string;
  ciUrl?: string;
  dashboardId?: string;
  stream?: string;
  ruleId?: string;
}

export interface ChangeListParams {
  from: number;
  to: number;
  services?: readonly string[] | undefined;
  namespace?: string | undefined;
  environment?: string | undefined;
  kinds?: readonly ChangeKind[] | undefined;
  dashboardId?: string | undefined;
  stream?: string | undefined;
  limit?: number | undefined;
}

/** A list moving within a minute asks for the same changes. */
const SLOT_MICROS = 60_000_000;

function query(params: ChangeListParams): Record<string, string | number> {
  const out: Record<string, string | number> = { from: params.from, to: params.to };
  if (params.services?.length) out.services = params.services.join(',');
  if (params.kinds?.length) out.kinds = params.kinds.join(',');
  for (const key of ['namespace', 'environment', 'dashboardId', 'stream', 'limit'] as const) {
    const value = params[key];
    if (value !== undefined && value !== '') out[key] = value;
  }
  return out;
}

export const changesApi = {
  list: async (params: ChangeListParams) => {
    const { data } = await http.get<ChangeEntry[]>('/changes', { params: query(params) });
    return data;
  },
};

export const changesQueryKeys = {
  list: (orgId: string, params: ChangeListParams) =>
    [
      'changes',
      orgId,
      Math.floor(params.from / SLOT_MICROS),
      Math.floor(params.to / SLOT_MICROS),
      params.services?.join(',') ?? null,
      params.namespace ?? null,
      params.environment ?? null,
      params.kinds?.join(',') ?? null,
      params.dashboardId ?? null,
      params.stream ?? null,
      params.limit ?? null,
    ] as const,
};
