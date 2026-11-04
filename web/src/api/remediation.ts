import { http } from '@/lib/http';

export type RemediationRisk = 'l2' | 'l3';
export type ParameterKind = 'text' | 'number' | 'select';
export type HttpVerb = 'GET' | 'POST' | 'PUT' | 'PATCH';
export type KubernetesOperation = 'restart' | 'scale' | 'rollback';
export type WorkloadKind = 'Deployment' | 'StatefulSet' | 'DaemonSet';
export type VerifyBy = 'incident_resolved' | 'rule_healthy';

export interface RemediationParameter {
  key: string;
  label: string;
  kind: ParameterKind;
  options: string[];
  required: boolean;
  default?: string | null;
}

export interface HttpTarget {
  kind: 'http';
  method: HttpVerb;
  url: string;
  /** Header names; their values never come back, and an empty value keeps the saved one. */
  headers?: Record<string, string>;
  body: string;
  /** Not given back; send one to set it, an empty one to clear it, none to keep it. */
  signingSecret?: string | null;
}

export interface KubernetesTarget {
  kind: 'kubernetes';
  clusterId: string;
  operation: KubernetesOperation;
  namespace: string;
  workloadKind: WorkloadKind;
  workloadName: string;
  replicas?: string | null;
}

export type RemediationTarget = HttpTarget | KubernetesTarget;

/** An action set up beforehand and run on an incident once approved. */
export interface RemediationAction {
  id: string;
  name: string;
  description: string;
  target: RemediationTarget;
  parameters: RemediationParameter[];
  services: string[];
  teamIds: string[];
  risk: RemediationRisk;
  /** Seconds after running to check it worked; 0 checks nothing. */
  verifyAfterSecs: number;
  verifyBy: VerifyBy;
  enabled: boolean;
  /** Which secrets are set: `signing_secret`, `header:<name>`. */
  secretsSet: string[];
  createdAt: number;
  updatedAt: number;
}

export type RemediationActionInput = Omit<RemediationAction, 'id' | 'secretsSet' | 'createdAt' | 'updatedAt'>;

export interface KubernetesCluster {
  id: string;
  name: string;
  apiServer: string;
  caCert: string;
  createdAt: number;
  updatedAt: number;
}

export interface KubernetesClusterInput {
  name: string;
  apiServer: string;
  caCert: string;
  /** Empty keeps the saved token. */
  token: string;
}

export type RemediationRunStatus =
  | 'awaiting_approval'
  | 'running'
  | 'succeeded'
  | 'failed'
  | 'declined'
  | 'verifying'
  | 'verified'
  | 'verification_failed';

/** An action started on an incident: its approval, what it did and how it was checked. */
export interface RemediationRun {
  id: string;
  incidentId: string;
  actionId: string;
  actionName: string;
  approvalId: string;
  inputs: Record<string, string>;
  status: RemediationRunStatus;
  output: string;
  verifyAt: number | null;
  verification: string;
  requestedBy: string;
  executedBy: string | null;
  createdAt: number;
  updatedAt: number;
}

export async function listActions(): Promise<RemediationAction[]> {
  const { data } = await http.get<{ actions: RemediationAction[] }>('/alerts/remediation/actions');
  return data.actions;
}

export async function createAction(input: RemediationActionInput): Promise<RemediationAction> {
  const { data } = await http.post<RemediationAction>('/alerts/remediation/actions', input);
  return data;
}

export async function updateAction(id: string, input: RemediationActionInput): Promise<RemediationAction> {
  const { data } = await http.put<RemediationAction>(`/alerts/remediation/actions/${encodeURIComponent(id)}`, input);
  return data;
}

export async function deleteAction(id: string) {
  await http.delete(`/alerts/remediation/actions/${encodeURIComponent(id)}`);
}

export async function listClusters(): Promise<KubernetesCluster[]> {
  const { data } = await http.get<{ clusters: KubernetesCluster[] }>('/alerts/remediation/clusters');
  return data.clusters;
}

export async function createCluster(input: KubernetesClusterInput): Promise<KubernetesCluster> {
  const { data } = await http.post<KubernetesCluster>('/alerts/remediation/clusters', input);
  return data;
}

export async function updateCluster(id: string, input: KubernetesClusterInput): Promise<KubernetesCluster> {
  const { data } = await http.put<KubernetesCluster>(`/alerts/remediation/clusters/${encodeURIComponent(id)}`, input);
  return data;
}

export async function deleteCluster(id: string) {
  await http.delete(`/alerts/remediation/clusters/${encodeURIComponent(id)}`);
}

export async function testCluster(id: string): Promise<{ ok: boolean; version: string }> {
  const { data } = await http.post<{ ok: boolean; version: string }>(
    `/alerts/remediation/clusters/${encodeURIComponent(id)}/test`,
  );
  return data;
}

/** The actions an incident can run and those started on it. */
export async function incidentRemediations(
  incidentId: string,
): Promise<{ actions: RemediationAction[]; runs: RemediationRun[] }> {
  const { data } = await http.get<{ actions: RemediationAction[]; runs: RemediationRun[] }>(
    `/alerts/incidents/${encodeURIComponent(incidentId)}/remediations`,
  );
  return data;
}

/** Starts an action on an incident: it waits for approval, then runs. */
export async function requestRemediation(
  incidentId: string,
  input: { actionId: string; inputs: Record<string, string>; reason: string },
): Promise<{ run: RemediationRun; approval: { id: string; requiredApprovals: number } }> {
  const { data } = await http.post<{ run: RemediationRun; approval: { id: string; requiredApprovals: number } }>(
    `/alerts/incidents/${encodeURIComponent(incidentId)}/remediations`,
    input,
  );
  return data;
}
