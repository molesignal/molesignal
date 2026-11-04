import { http } from '@/lib/http';

import type { RiskLevel } from './automations';

export type ToolExecutionMode =
  | 'automatic'
  | 'confirmation'
  | 'single_approval'
  | 'dual_approval'
  | 'disabled';

export type ToolDomain =
  | 'observability'
  | 'alerts_on_call'
  | 'automation'
  | 'knowledge_context'
  | 'dashboard_reports'
  | 'notify'
  | 'administration';

export type ToolStatus = 'healthy' | 'degraded' | 'unavailable' | 'disabled';

export interface ToolSource {
  kind: 'builtin' | 'mcp' | 'custom';
  label: string;
  serverId?: string;
  serverName?: string;
}

export interface ToolCapabilities {
  readOnly: boolean;
  supportsDryRun: boolean;
  idempotent: boolean;
  streaming: boolean;
}

export interface ToolLimits {
  timeoutMs: number;
  maxCallsPerRun: number;
  maxResponseBytes: number;
}

export interface ToolStatistics {
  calls24h: number;
  successRate?: number | null;
  p95Ms?: number | null;
  lastCalledAt?: number | null;
  lastError?: string | null;
}

export interface RegisteredTool {
  id: string;
  name: string;
  remoteName?: string;
  displayName: string;
  description: string;
  technicalDescription: string;
  domain: ToolDomain;
  category: string;
  source: ToolSource;
  inputSchema: Record<string, unknown>;
  outputSchema?: Record<string, unknown> | null;
  risk: RiskLevel;
  minimumRisk?: RiskLevel;
  executionMode: ToolExecutionMode;
  enabled: boolean;
  availableToAgent: boolean;
  status: ToolStatus;
  capabilities: ToolCapabilities;
  limits: ToolLimits;
  environmentOverrides: Record<string, ToolExecutionMode>;
  tags: string[];
  statistics: ToolStatistics;
  lastSyncedAt?: number | null;
  version?: string | null;
  access:
    | 'read_only'
    | 'preflight'
    | 'managed_mutation'
    | 'creates_approval_request'
    | 'executes_approved_operation'
    | 'chat_record';
}

export interface ToolRegistry {
  tools: RegisteredTool[];
  dynamicHttp: boolean;
  shell: boolean;
  browser: boolean;
  openMcp: boolean;
  mcpServers?: {
    total: number;
    healthy: number;
    unhealthy: number;
  };
}

export interface ToolPolicyInput {
  enabled?: boolean;
  risk?: RiskLevel;
  executionMode?: ToolExecutionMode;
  environmentOverrides?: Record<string, ToolExecutionMode>;
  timeoutMs?: number;
  maxCallsPerRun?: number;
  maxResponseBytes?: number;
}

export interface ToolDependencies {
  toolName: string;
  total: number;
  agentProfiles: Array<{
    id: string;
    name: string;
    enabled: boolean;
    isDefault: boolean;
  }>;
  investigationTemplates: Array<{ id: string; name: string }>;
}

export interface ToolCallRecord {
  id: string;
  toolName: string;
  chatId?: string | null;
  investigationId?: string | null;
  risk: RiskLevel;
  input: Record<string, unknown>;
  outputSummary?: string | null;
  status: string;
  error?: string | null;
  durationMs: number;
  calledBy: string;
  callSource: 'chat' | 'investigation' | 'automation' | 'manual_test' | string;
  profileId?: string | null;
  approvalId?: string | null;
  policyDecision: Record<string, unknown>;
  auditId?: string | null;
  createdAt: number;
}

export interface ToolTestResult {
  success: boolean;
  validated: boolean;
  dryRun: boolean;
  executed: boolean;
  sideEffects: boolean;
  durationMs?: number;
  message?: string;
  request?: Record<string, unknown>;
  response?: unknown;
}

export interface ToolPolicyDefaults {
  orgId: string;
  riskModes: Record<RiskLevel, ToolExecutionMode>;
  environmentOverrides: Record<string, Partial<Record<RiskLevel, ToolExecutionMode>>>;
  updatedBy: string;
  createdAt: number;
  updatedAt: number;
}

export async function listTools(): Promise<ToolRegistry> {
  const { data } = await http.get<ToolRegistry>('/agent/tools');
  return data;
}

export async function getTool(id: string): Promise<{
  tool: RegisteredTool;
  dependencies: ToolDependencies;
}> {
  const { data } = await http.get<{
    tool: RegisteredTool;
    dependencies: ToolDependencies;
  }>(`/agent/tools/${encodeURIComponent(id)}`);
  return data;
}

export async function updateToolPolicy(
  id: string,
  input: ToolPolicyInput,
): Promise<RegisteredTool> {
  const { data } = await http.put<RegisteredTool>(
    `/agent/tools/${encodeURIComponent(id)}/policy`,
    input,
  );
  return data;
}

export async function enableTool(id: string): Promise<RegisteredTool> {
  const { data } = await http.post<RegisteredTool>(
    `/agent/tools/${encodeURIComponent(id)}/enable`,
  );
  return data;
}

export async function disableTool(
  id: string,
  force = false,
): Promise<{ tool: RegisteredTool; dependencies: ToolDependencies }> {
  const { data } = await http.post<{
    tool: RegisteredTool;
    dependencies: ToolDependencies;
  }>(`/agent/tools/${encodeURIComponent(id)}/disable`, { force });
  return data;
}

export async function getToolDependencies(id: string): Promise<ToolDependencies> {
  const { data } = await http.get<ToolDependencies>(
    `/agent/tools/${encodeURIComponent(id)}/dependencies`,
  );
  return data;
}

export async function listToolCalls(id: string, limit = 100): Promise<ToolCallRecord[]> {
  const { data } = await http.get<{ calls: ToolCallRecord[] }>(
    `/agent/tools/${encodeURIComponent(id)}/calls`,
    { params: { limit } },
  );
  return data.calls ?? [];
}

export async function testTool(
  id: string,
  input: {
    arguments: Record<string, unknown>;
    dryRun?: boolean;
    validateOnly?: boolean;
  },
): Promise<ToolTestResult> {
  const { data } = await http.post<ToolTestResult>(
    `/agent/tools/${encodeURIComponent(id)}/test`,
    input,
  );
  return data;
}

export async function getToolPolicyDefaults(): Promise<ToolPolicyDefaults> {
  const { data } = await http.get<ToolPolicyDefaults>('/agent/tools/policies');
  return data;
}

export async function updateToolPolicyDefaults(input: {
  riskModes: Record<RiskLevel, ToolExecutionMode>;
  environmentOverrides: Record<string, Partial<Record<RiskLevel, ToolExecutionMode>>>;
}): Promise<ToolPolicyDefaults> {
  const { data } = await http.put<ToolPolicyDefaults>('/agent/tools/policies', input);
  return data;
}
