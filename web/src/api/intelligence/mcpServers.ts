import { http } from '@/lib/http';

import type { RegisteredTool } from './tools';

export type McpTransport = 'streamable_http' | 'sse' | 'stdio' | 'unix_socket';
export type McpAuthType =
  | 'none'
  | 'bearer_token'
  | 'api_key'
  | 'oauth'
  | 'mtls'
  | 'internal_service_identity';

export interface McpServer {
  id: string;
  org_id: string;
  name: string;
  transport: McpTransport;
  endpoint_url?: string | null;
  command_template?: string | null;
  auth_type: McpAuthType;
  auth_header?: string | null;
  credential_last4?: string | null;
  credential_set: boolean;
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
}

export interface McpServerInput {
  name: string;
  transport: McpTransport;
  endpoint_url?: string;
  command_template?: string;
  auth_type: McpAuthType;
  auth_header?: string;
  credential?: string;
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

export interface McpTestResult {
  success: boolean;
  server: McpServer;
  discovered_tools: DiscoveredMcpTool[];
  error?: string;
}

export async function listMcpServers(): Promise<McpServer[]> {
  const { data } = await http.get<{ servers: McpServer[] }>('/intelligence/mcp-servers');
  return data.servers ?? [];
}

export async function getMcpServer(id: string): Promise<{
  server: McpServer;
  tools: RegisteredTool[];
}> {
  const { data } = await http.get<{
    server: McpServer;
    tools: RegisteredTool[];
  }>(`/intelligence/mcp-servers/${encodeURIComponent(id)}`);
  return data;
}

export async function createMcpServer(input: McpServerInput): Promise<McpServer> {
  const { data } = await http.post<McpServer>('/intelligence/mcp-servers', input);
  return data;
}

export async function updateMcpServer(
  id: string,
  input: McpServerInput,
): Promise<McpServer> {
  const { data } = await http.put<McpServer>(
    `/intelligence/mcp-servers/${encodeURIComponent(id)}`,
    input,
  );
  return data;
}

export async function deleteMcpServer(id: string): Promise<void> {
  await http.delete(`/intelligence/mcp-servers/${encodeURIComponent(id)}`);
}

export async function testMcpServer(id: string): Promise<McpTestResult> {
  const { data } = await http.post<McpTestResult>(
    `/intelligence/mcp-servers/${encodeURIComponent(id)}/test`,
  );
  return data;
}

export async function syncMcpServer(
  id: string,
  selectedTools: string[],
): Promise<{ server: McpServer; tools: RegisteredTool[] }> {
  const { data } = await http.post<{
    server: McpServer;
    tools: RegisteredTool[];
  }>(`/intelligence/mcp-servers/${encodeURIComponent(id)}/sync`, {
    selected_tools: selectedTools,
  });
  return data;
}
