import type {
  HttpVerb,
  KubernetesOperation,
  ParameterKind,
  RemediationAction,
  RemediationActionInput,
  RemediationRisk,
  VerifyBy,
  WorkloadKind,
} from '@/api/remediation';

export const ACTIONS_KEY = ['remediation-actions'];
export const CLUSTERS_KEY = ['kubernetes-clusters'];

export interface ParameterRow {
  key: string;
  label: string;
  kind: ParameterKind;
  /** Options as typed, comma separated. */
  options: string;
  required: boolean;
  default: string;
}

export interface HeaderRow {
  name: string;
  /** Empty keeps a saved value. */
  value: string;
}

/** An action as the drawer edits it. */
export interface ActionDraft {
  name: string;
  description: string;
  kind: 'http' | 'kubernetes';
  method: HttpVerb;
  url: string;
  headers: HeaderRow[];
  body: string;
  /** Empty keeps the saved secret; `clearSecret` removes it. */
  signingSecret: string;
  clearSecret: boolean;
  clusterId: string;
  operation: KubernetesOperation;
  workloadKind: WorkloadKind;
  namespace: string;
  workloadName: string;
  replicas: string;
  parameters: ParameterRow[];
  services: string;
  risk: RemediationRisk;
  verifyMinutes: number;
  verifyBy: VerifyBy;
  enabled: boolean;
}

export function draftOf(action: RemediationAction | null): ActionDraft {
  const http = action?.target.kind === 'http' ? action.target : null;
  const kubernetes = action?.target.kind === 'kubernetes' ? action.target : null;

  return {
    name: action?.name ?? '',
    description: action?.description ?? '',
    kind: action?.target.kind ?? 'http',
    method: http?.method ?? 'POST',
    url: http?.url ?? '',
    headers: Object.keys(http?.headers ?? {}).map((name) => ({ name, value: '' })),
    body: http?.body ?? '',
    signingSecret: '',
    clearSecret: false,
    clusterId: kubernetes?.clusterId ?? '',
    operation: kubernetes?.operation ?? 'restart',
    workloadKind: kubernetes?.workloadKind ?? 'Deployment',
    namespace: kubernetes?.namespace ?? '',
    workloadName: kubernetes?.workloadName ?? '{{incident.service}}',
    replicas: kubernetes?.replicas ?? '{{params.replicas}}',
    parameters: (action?.parameters ?? []).map((parameter) => ({
      key: parameter.key,
      label: parameter.label,
      kind: parameter.kind,
      options: parameter.options.join(', '),
      required: parameter.required,
      default: parameter.default ?? '',
    })),
    services: (action?.services ?? []).join(', '),
    risk: action?.risk ?? 'l2',
    verifyMinutes: Math.round((action?.verifyAfterSecs ?? 0) / 60),
    verifyBy: action?.verifyBy ?? 'incident_resolved',
    enabled: action?.enabled ?? true,
  };
}

const words = (text: string) =>
  text
    .split(',')
    .map((word) => word.trim())
    .filter(Boolean);

/** What the server takes; for an HTTP action, an empty header value keeps the saved one. */
export function inputOf(draft: ActionDraft, teamIds: string[]): RemediationActionInput {
  return {
    name: draft.name.trim(),
    description: draft.description.trim(),
    target:
      draft.kind === 'http'
        ? {
            kind: 'http',
            method: draft.method,
            url: draft.url.trim(),
            headers: Object.fromEntries(
              draft.headers.filter((row) => row.name.trim()).map((row) => [row.name.trim(), row.value]),
            ),
            body: draft.body,
            ...(draft.clearSecret
              ? { signingSecret: '' }
              : draft.signingSecret.trim()
                ? { signingSecret: draft.signingSecret.trim() }
                : {}),
          }
        : {
            kind: 'kubernetes',
            clusterId: draft.clusterId,
            operation: draft.operation,
            namespace: draft.namespace.trim(),
            workloadKind: draft.workloadKind,
            workloadName: draft.workloadName.trim(),
            replicas: draft.operation === 'scale' ? draft.replicas.trim() : null,
          },
    parameters: draft.parameters
      .filter((row) => row.key.trim())
      .map((row) => ({
        key: row.key.trim(),
        label: row.label.trim() || row.key.trim(),
        kind: row.kind,
        options: row.kind === 'select' ? words(row.options) : [],
        required: row.required,
        default: row.default.trim() || null,
      })),
    services: words(draft.services),
    teamIds: teamIds,
    risk: draft.risk,
    verifyAfterSecs: Math.max(0, Math.round(draft.verifyMinutes)) * 60,
    verifyBy: draft.verifyBy,
    enabled: draft.enabled,
  };
}

/** Whether a body template reads as JSON once filled in: every `{{name}}` counts as `0`, so a placeholder in
 * quotes and one used bare as a number (`{"replicas": {{params.replicas}}}`) both do. An empty body is fine. */
export function bodyReadsAsJson(body: string): boolean {
  if (!body.trim()) return true;

  try {
    JSON.parse(body.replace(/\{\{[^}]*\}\}/g, '0'));
    return true;
  } catch {
    return false;
  }
}

/** A line saying what an action does. */
export function targetSummary(action: RemediationAction, clusterName: (id: string) => string | undefined): string {
  if (action.target.kind === 'http') return `${action.target.method} ${action.target.url}`;

  const target = action.target;
  return `${target.operation} ${target.workloadKind} ${target.namespace}/${target.workloadName} · ${
    clusterName(target.clusterId) ?? target.clusterId
  }`;
}
