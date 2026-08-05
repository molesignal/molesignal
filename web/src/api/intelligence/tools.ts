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
  server_id?: string;
  server_name?: string;
}

export interface ToolCapabilities {
  read_only: boolean;
  supports_dry_run: boolean;
  idempotent: boolean;
  streaming: boolean;
}

export interface ToolLimits {
  timeout_ms: number;
  max_calls_per_run: number;
  max_response_bytes: number;
}

export interface ToolStatistics {
  calls_24h: number;
  success_rate?: number | null;
  p95_ms?: number | null;
  last_called_at?: number | null;
  last_error?: string | null;
}

export interface RegisteredTool {
  id: string;
  name: string;
  remote_name?: string;
  display_name: string;
  description: string;
  technical_description: string;
  domain: ToolDomain;
  category: string;
  source: ToolSource;
  input_schema: Record<string, unknown>;
  output_schema?: Record<string, unknown> | null;
  risk: RiskLevel;
  minimum_risk?: RiskLevel;
  execution_mode: ToolExecutionMode;
  enabled: boolean;
  available_to_agent: boolean;
  status: ToolStatus;
  capabilities: ToolCapabilities;
  limits: ToolLimits;
  environment_overrides: Record<string, ToolExecutionMode>;
  tags: string[];
  statistics: ToolStatistics;
  last_synced_at?: number | null;
  version?: string | null;
  access: 'read_only' | 'creates_approval_request';
}

export interface ToolRegistry {
  tools: RegisteredTool[];
  dynamic_http: boolean;
  shell: boolean;
  browser: boolean;
  open_mcp: boolean;
  mcp_servers?: {
    total: number;
    healthy: number;
    unhealthy: number;
  };
}

export interface ToolPolicyInput {
  enabled?: boolean;
  risk?: RiskLevel;
  execution_mode?: ToolExecutionMode;
  environment_overrides?: Record<string, ToolExecutionMode>;
  timeout_ms?: number;
  max_calls_per_run?: number;
  max_response_bytes?: number;
}

export interface ToolDependencies {
  tool_name: string;
  total: number;
  agent_profiles: Array<{
    id: string;
    name: string;
    enabled: boolean;
    is_default: boolean;
  }>;
  automations: Array<{ id: string; name: string; enabled: boolean }>;
  investigation_templates: Array<{ id: string; name: string }>;
}

export interface ToolCallRecord {
  id: string;
  tool_name: string;
  chat_id?: string | null;
  investigation_id?: string | null;
  risk: RiskLevel;
  input: Record<string, unknown>;
  output_summary?: string | null;
  status: string;
  error?: string | null;
  duration_ms: number;
  called_by: string;
  call_source: 'chat' | 'investigation' | 'automation' | 'manual_test' | string;
  profile_id?: string | null;
  approval_id?: string | null;
  policy_decision: Record<string, unknown>;
  audit_id?: string | null;
  created_at: number;
}

export interface ToolTestResult {
  success: boolean;
  validated: boolean;
  dry_run: boolean;
  executed: boolean;
  side_effects: boolean;
  duration_ms?: number;
  message?: string;
  request?: Record<string, unknown>;
  response?: unknown;
}

export interface ToolPolicyDefaults {
  org_id: string;
  risk_modes: Record<RiskLevel, ToolExecutionMode>;
  environment_overrides: Record<string, Partial<Record<RiskLevel, ToolExecutionMode>>>;
  updated_by: string;
  created_at: number;
  updated_at: number;
}

export async function listTools(): Promise<ToolRegistry> {
  const { data } = await http.get<ToolRegistry>('/intelligence/tools');
  return data;
}

export async function getTool(id: string): Promise<{
  tool: RegisteredTool;
  dependencies: ToolDependencies;
}> {
  const { data } = await http.get<{
    tool: RegisteredTool;
    dependencies: ToolDependencies;
  }>(`/intelligence/tools/${encodeURIComponent(id)}`);
  return data;
}

export async function updateToolPolicy(
  id: string,
  input: ToolPolicyInput,
): Promise<RegisteredTool> {
  const { data } = await http.put<RegisteredTool>(
    `/intelligence/tools/${encodeURIComponent(id)}/policy`,
    input,
  );
  return data;
}

export async function enableTool(id: string): Promise<RegisteredTool> {
  const { data } = await http.post<RegisteredTool>(
    `/intelligence/tools/${encodeURIComponent(id)}/enable`,
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
  }>(`/intelligence/tools/${encodeURIComponent(id)}/disable`, { force });
  return data;
}

export async function getToolDependencies(id: string): Promise<ToolDependencies> {
  const { data } = await http.get<ToolDependencies>(
    `/intelligence/tools/${encodeURIComponent(id)}/dependencies`,
  );
  return data;
}

export async function listToolCalls(id: string, limit = 100): Promise<ToolCallRecord[]> {
  const { data } = await http.get<{ calls: ToolCallRecord[] }>(
    `/intelligence/tools/${encodeURIComponent(id)}/calls`,
    { params: { limit } },
  );
  return data.calls ?? [];
}

export async function testTool(
  id: string,
  input: {
    arguments: Record<string, unknown>;
    dry_run?: boolean;
    validate_only?: boolean;
  },
): Promise<ToolTestResult> {
  const { data } = await http.post<ToolTestResult>(
    `/intelligence/tools/${encodeURIComponent(id)}/test`,
    input,
  );
  return data;
}

export async function getToolPolicyDefaults(): Promise<ToolPolicyDefaults> {
  const { data } = await http.get<ToolPolicyDefaults>('/intelligence/tools/policies');
  return data;
}

export async function updateToolPolicyDefaults(input: {
  risk_modes: Record<RiskLevel, ToolExecutionMode>;
  environment_overrides: Record<string, Partial<Record<RiskLevel, ToolExecutionMode>>>;
}): Promise<ToolPolicyDefaults> {
  const { data } = await http.put<ToolPolicyDefaults>('/intelligence/tools/policies', input);
  return data;
}
