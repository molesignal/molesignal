import { http } from '@/lib/http';

/** The closed set of tasks a Node Agent runs. There is no "run a command". */
export const NODE_TASK_KINDS = ['autotracing_run', 'profile', 'tool'] as const;
export type NodeTaskKind = (typeof NODE_TASK_KINDS)[number];

export type NodeStatus = 'enrolled' | 'online' | 'offline' | 'draining' | 'drained' | 'revoked';

export const NODE_TASK_STATES = [
  'queued',
  'offered',
  'running',
  'succeeded',
  'failed',
  'cancelled',
  'lease_expired',
  'expired',
  'rejected',
] as const;
export type NodeTaskState = (typeof NODE_TASK_STATES)[number];

export interface NodeCapabilities {
  taskKinds: NodeTaskKind[];
  features: string[];
  maxRunningTasks: number;
}

export interface NodeHealth {
  state: 'unknown' | 'healthy' | 'degraded';
  messages: string[];
}

export interface NodeCollectorState {
  name: string;
  state: string;
  message: string;
}

export interface NodeDesiredConfig {
  revision: number;
  config: Record<string, unknown>;
  updatedBy: string | null;
  updatedAt: number;
}

export interface NodeConfigStatus {
  revision: number;
  state: 'applied' | 'rejected' | 'rolled_back';
  reason: string | null;
  reportedAt: number;
}

/** A node of the inventory, with the status it has now. Times are microseconds. */
export interface NodeAgent {
  id: string;
  orgId: string;
  hostId: string;
  bootId: string;
  hostname: string;
  labels: Record<string, string>;
  agentVersion: string;
  protocolVersion: string;
  capabilities: NodeCapabilities;
  certificateSerial: string;
  certificateExpiresAt: number;
  enrollmentTokenId: string | null;
  health: NodeHealth;
  collectorStates: NodeCollectorState[];
  appliedConfigRevision: number;
  desiredConfig: NodeDesiredConfig | null;
  configStatus: NodeConfigStatus | null;
  lastHeartbeatAt: number | null;
  lastConnectedAt: number | null;
  drainRequestedAt: number | null;
  drainDeadline: number | null;
  drainedAt: number | null;
  revokedAt: number | null;
  revokeReason: string | null;
  enrolledAt: number;
  createdAt: number;
  updatedAt: number;
  status: NodeStatus;
}

export interface EnrollmentToken {
  id: string;
  orgId: string;
  description: string;
  labels: Record<string, string>;
  maxUses: number;
  uses: number;
  expiresAt: number;
  revokedAt: number | null;
  lastUsedAt: number | null;
  createdBy: string;
  createdAt: number;
}

/** A token as issued: the one time its plaintext is shown. */
export interface IssuedEnrollmentToken extends EnrollmentToken {
  plaintext: string;
  endpoint: string;
  caCertificatePem: string;
}

export interface CreateEnrollmentTokenPayload {
  description: string;
  labels: Record<string, string>;
  maxUses: number;
  ttlMinutes: number;
}

export interface NodeTaskOutcome {
  resultId: string;
  attempt: number;
  status: 'succeeded' | 'failed' | 'cancelled' | 'lease_expired';
  summary: string;
  artifactRefs: string[];
  receivedAt: number;
}

export interface NodeTask {
  id: string;
  orgId: string;
  nodeId: string;
  kind: NodeTaskKind;
  spec: Record<string, unknown>;
  state: NodeTaskState;
  attempt: number;
  maxAttempts: number;
  availableAt: number;
  leaseExpiresAt: number | null;
  deadline: number;
  cancelRequestedAt: number | null;
  cancelRequestedBy: string | null;
  result: NodeTaskOutcome | null;
  failure: string | null;
  createdBy: string;
  createdAt: number;
  updatedAt: number;
  finishedAt: number | null;
}

export interface CreateNodeTaskPayload {
  nodeId: string;
  kind: NodeTaskKind;
  spec: Record<string, unknown>;
  timeoutSeconds: number;
  maxAttempts: number;
}

export interface NodeTaskQuota {
  maxActivePerOrg: number;
  maxActivePerNode: number;
  maxCreatedPerHourPerOrg: number;
  maxCreatedPerHourPerNode: number;
}

export interface NodeTaskFilter {
  nodeId?: string | undefined;
  states?: readonly NodeTaskState[] | undefined;
  limit?: number | undefined;
}

const ACTIVE_TASK_STATES: readonly NodeTaskState[] = ['queued', 'offered', 'running'];

export function isActiveTask(task: Pick<NodeTask, 'state'>): boolean {
  return ACTIVE_TASK_STATES.includes(task.state);
}

const path = (id: string) => encodeURIComponent(id);

export const nodeAgentKeys = {
  all: ['node-agents'] as const,
  nodes: () => ['node-agents', 'nodes'] as const,
  tokens: () => ['node-agents', 'enrollment-tokens'] as const,
  tasks: (filter: NodeTaskFilter = {}) => ['node-agents', 'tasks', filter] as const,
  quota: () => ['node-agents', 'task-quota'] as const,
};

export async function listNodes(): Promise<NodeAgent[]> {
  const { data } = await http.get<NodeAgent[]>('/node-agents/nodes');
  return data;
}

export async function drainNode(nodeId: string, deadlineSeconds: number): Promise<NodeAgent> {
  const { data } = await http.post<NodeAgent>(`/node-agents/nodes/${path(nodeId)}/drain`, {
    deadlineSeconds,
  });
  return data;
}

export async function resumeNode(nodeId: string): Promise<NodeAgent> {
  const { data } = await http.post<NodeAgent>(`/node-agents/nodes/${path(nodeId)}/resume`, {});
  return data;
}

export async function revokeNode(nodeId: string, reason: string): Promise<NodeAgent> {
  const { data } = await http.post<NodeAgent>(`/node-agents/nodes/${path(nodeId)}/revoke`, {
    reason,
  });
  return data;
}

export async function setNodeConfig(
  nodeId: string,
  config: Record<string, unknown>,
): Promise<NodeAgent> {
  const { data } = await http.put<NodeAgent>(`/node-agents/nodes/${path(nodeId)}/config`, {
    config,
  });
  return data;
}

export async function listEnrollmentTokens(): Promise<EnrollmentToken[]> {
  const { data } = await http.get<EnrollmentToken[]>('/node-agents/enrollment-tokens');
  return data;
}

export async function createEnrollmentToken(
  payload: CreateEnrollmentTokenPayload,
): Promise<IssuedEnrollmentToken> {
  const { data } = await http.post<IssuedEnrollmentToken>('/node-agents/enrollment-tokens', payload);
  return data;
}

export async function revokeEnrollmentToken(tokenId: string): Promise<EnrollmentToken> {
  const { data } = await http.post<EnrollmentToken>(
    `/node-agents/enrollment-tokens/${path(tokenId)}/revoke`,
    {},
  );
  return data;
}

export async function listTasks(filter: NodeTaskFilter = {}): Promise<NodeTask[]> {
  const params: Record<string, string> = {};
  if (filter.nodeId) params.nodeId = filter.nodeId;
  if (filter.states?.length) params.state = filter.states.join(',');
  if (filter.limit) params.limit = String(filter.limit);
  const { data } = await http.get<NodeTask[]>('/node-agents/tasks', { params });
  return data;
}

export async function createTask(payload: CreateNodeTaskPayload): Promise<NodeTask> {
  const { data } = await http.post<NodeTask>('/node-agents/tasks', payload);
  return data;
}

export async function cancelTask(taskId: string): Promise<NodeTask> {
  const { data } = await http.post<NodeTask>(`/node-agents/tasks/${path(taskId)}/cancel`, {});
  return data;
}

export async function getTaskQuota(): Promise<NodeTaskQuota> {
  const { data } = await http.get<NodeTaskQuota>('/node-agents/task-quota');
  return data;
}

export async function setTaskQuota(quota: NodeTaskQuota): Promise<NodeTaskQuota> {
  const { data } = await http.put<NodeTaskQuota>('/node-agents/task-quota', quota);
  return data;
}
