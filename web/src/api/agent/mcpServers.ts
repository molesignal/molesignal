import { http } from '@/lib/http';

import type { RiskLevel } from './automations';
import type { ToolExecutionMode } from './tools';

export type McpTransport = 'streamable_http' | 'sse' | 'stdio' | 'unix_socket';
export type McpAuthType =
  | 'none'
  | 'bearer_token'
  | 'api_key'
  | 'oauth'
  | 'mtls'
  | 'internal_service_identity'
  | 'member_secret';

/** Whose an MCP server is: the organization's, a team's, or one person's. */
export type McpScope =
  | { scope: 'org' }
  | { scope: 'team'; team_id: string }
  | { scope: 'personal'; owner_id: string };

/**
 * A header or an environment variable of a server. A secret one never comes
 * back: only whether it is filled in and its last four characters.
 */
export interface McpNamedValue {
  name: string;
  secret: boolean;
  value?: string | null;
  last4?: string | null;
  set: boolean;
}

/** A header or an environment variable as it is saved: a secret left empty keeps its value. */
export interface McpNamedValueInput {
  name: string;
  secret: boolean;
  value?: string;
}

/** Where each member's own credential goes, for servers that take one from every member. */
export interface McpMemberCredentialSpec {
  /** The header, in lower case. */
  header: string;
  /** Sent as `Bearer <value>`. */
  bearer: boolean;
  /** Where members get their credential and which permissions it needs. */
  instructions: string;
}

/** A credential a member saved for a server: only the last four characters. */
export interface McpMemberCredential {
  server_id: string;
  user_id: string;
  last4: string;
  updated_at: number;
}

export interface McpServer {
  id: string;
  org_id: string;
  scope: McpScope;
  /** The prefix of its tools and prompt commands, unique in the organization. */
  handle: string;
  name: string;
  transport: McpTransport;
  endpoint_url?: string | null;
  command_template?: string | null;
  auth_type: McpAuthType;
  auth_header?: string | null;
  credential_last4?: string | null;
  credential_set: boolean;
  member_credential?: McpMemberCredentialSpec | null;
  headers: McpNamedValue[];
  env: McpNamedValue[];
  private_only: boolean;
  allowed_domains: string[];
  allowed_cidrs: string[];
  follow_redirects: boolean;
  tls_verify: boolean;
  timeout_ms: number;
  max_response_bytes: number;
  enabled: boolean;
  status: 'healthy' | 'connecting' | 'error' | 'disabled' | 'unauthorized' | 'unavailable';
  last_error?: string | null;
  last_tested_at?: number | null;
  last_synced_at?: number | null;
  created_by: string;
  created_at: number;
  updated_at: number;
  tool_count?: number;
  prompt_count?: number;
  /** The one who reads can change it. */
  editable?: boolean;
  /** The one who reads can disable or remove it. */
  moderatable?: boolean;
  /** The credential the one who reads saved for it. */
  my_credential?: McpMemberCredential | null;
}

export interface McpServerInput {
  scope: 'org' | 'team' | 'personal';
  team_id?: string;
  name: string;
  transport: McpTransport;
  endpoint_url?: string;
  command_template?: string;
  auth_type: McpAuthType;
  auth_header?: string;
  credential?: string;
  member_credential?: McpMemberCredentialSpec;
  headers: McpNamedValueInput[];
  env: McpNamedValueInput[];
  private_only: boolean;
  allowed_domains: string[];
  allowed_cidrs: string[];
  follow_redirects: boolean;
  tls_verify: boolean;
  timeout_ms: number;
  max_response_bytes: number;
  enabled: boolean;
}

export interface DiscoveredMcpTool {
  name: string;
  title?: string | null;
  description: string;
  inputSchema: Record<string, unknown>;
  outputSchema?: Record<string, unknown> | null;
  annotations?: Record<string, unknown>;
}

/** A tool of a server as it is registered: off until someone turns it on. */
export interface McpToolRow {
  id: string;
  org_id: string;
  server_id: string;
  /** The name on the server. */
  remote_name: string;
  /** The name in Mole Agent: `mcp__<handle>__<tool>`. */
  name: string;
  display_name: string;
  description: string;
  minimum_risk: RiskLevel;
  risk: RiskLevel;
  execution_mode: ToolExecutionMode;
  capabilities: { read_only?: boolean; idempotent?: boolean } & Record<string, unknown>;
  enabled: boolean;
  status: 'healthy' | 'degraded' | 'unavailable' | 'disabled';
  /** Why the tool cannot be used, for example its schema changed on the server. */
  unavailable_diagnostic?: string | null;
  last_synced_at: number;
}

export interface McpPromptArgument {
  name: string;
  description: string;
  required: boolean;
}

/** A prompt a server offers; the chat runs it as `/<handle>:<prompt>`. */
export interface McpPrompt {
  org_id: string;
  server_id: string;
  name: string;
  title?: string | null;
  description: string;
  arguments: McpPromptArgument[];
  synced_at: number;
}

export interface McpTestResult {
  success: boolean;
  server: McpServer;
  discovered_tools: DiscoveredMcpTool[];
  error?: string;
}

export interface McpImportReport {
  /** The servers made, each with the headers and variables still to fill in. */
  created: Array<McpServer & { missing: string[] }>;
  skipped: Array<{ name: string; reason: string }>;
}

const path = (id: string) => `/agent/mcp-servers/${encodeURIComponent(id)}`;

export async function listMcpServers(): Promise<McpServer[]> {
  const { data } = await http.get<{ servers: McpServer[] }>('/agent/mcp-servers');
  return data.servers ?? [];
}

/**
 * The programs this MoleSignal instance lets the organization's stdio servers
 * start (`[agent] mcp_stdio_commands`); none means stdio servers do not run.
 */
export async function mcpStdioCommands(): Promise<string[]> {
  const { data } = await http.get<{ stdio_commands?: string[] }>('/agent/mcp-servers');
  return data.stdio_commands ?? [];
}

/** The program a stdio command line starts: its first word, quotes taken off. */
export function stdioProgram(command: string): string {
  const match = /^\s*(?:'([^']*)'|"([^"]*)"|(\S+))/.exec(command);
  return match?.[1] ?? match?.[2] ?? match?.[3] ?? '';
}

export async function getMcpServer(id: string): Promise<{
  server: McpServer;
  tools: McpToolRow[];
  prompts: McpPrompt[];
}> {
  const { data } = await http.get<{
    server: McpServer;
    tools: McpToolRow[];
    prompts?: McpPrompt[];
  }>(path(id));
  return { ...data, prompts: data.prompts ?? [] };
}

export async function createMcpServer(input: McpServerInput): Promise<McpServer> {
  const { data } = await http.post<McpServer>('/agent/mcp-servers', input);
  return data;
}

export async function updateMcpServer(
  id: string,
  input: McpServerInput,
): Promise<McpServer> {
  const { data } = await http.put<McpServer>(path(id), input);
  return data;
}

/** Turns a server on or off; also for administrators on servers they cannot edit. */
export async function setMcpServerEnabled(id: string, enabled: boolean): Promise<McpServer> {
  const { data } = await http.patch<McpServer>(path(id), { enabled });
  return data;
}

export async function deleteMcpServer(id: string): Promise<void> {
  await http.delete(path(id));
}

export async function testMcpServer(id: string): Promise<McpTestResult> {
  const { data } = await http.post<McpTestResult>(`${path(id)}/test`);
  return data;
}

export async function syncMcpServer(
  id: string,
  selectedTools: string[],
): Promise<{ server: McpServer; tools: McpToolRow[]; prompt_count?: number }> {
  const { data } = await http.post<{
    server: McpServer;
    tools: McpToolRow[];
    prompt_count?: number;
  }>(`${path(id)}/sync`, {
    selected_tools: selectedTools,
  });
  return data;
}

/** Turns on every read-only tool of a server that is still off. */
export async function enableReadOnlyMcpTools(
  id: string,
): Promise<{ enabled: number; tools: McpToolRow[] }> {
  const { data } = await http.post<{ enabled: number; tools: McpToolRow[] }>(
    `${path(id)}/enable-read-only`,
  );
  return data;
}

/** Turns one tool of a server on or off. */
export async function setMcpToolEnabled(
  serverId: string,
  toolId: string,
  enabled: boolean,
): Promise<McpToolRow> {
  const { data } = await http.patch<McpToolRow>(
    `${path(serverId)}/tools/${encodeURIComponent(toolId)}`,
    { enabled },
  );
  return data;
}

/** Saves the signed-in member's own credential for a server. */
export async function putMyMcpCredential(
  id: string,
  value: string,
): Promise<McpMemberCredential> {
  const { data } = await http.put<{ credential: McpMemberCredential }>(
    `${path(id)}/my-credential`,
    { value },
  );
  return data.credential;
}

export async function deleteMyMcpCredential(id: string): Promise<boolean> {
  const { data } = await http.delete<{ deleted: boolean }>(`${path(id)}/my-credential`);
  return data.deleted;
}

/** Makes servers out of the `mcpServers` configuration of another client, pasted as text. */
export async function importMcpServers(input: {
  scope: 'org' | 'team' | 'personal';
  team_id?: string;
  config: string;
}): Promise<McpImportReport> {
  const { data } = await http.post<McpImportReport>('/agent/mcp-servers/import', input);
  return data;
}
