import { http } from '@/lib/http';

export interface InboundMcpSettings {
  orgId: string;
  enabled: boolean;
  allowedOrigins: string[];
  /** MiB a request body may take. */
  maxRequestSize: number;
  /** MiB a response may take. */
  maxResponseSize: number;
  maxConcurrentCalls: number;
  callsPerMinute: number;
  readTimeoutMs: number;
  updatedBy: string;
  createdAt: number;
  updatedAt: number;
}

export type InboundMcpSettingsInput = Pick<
  InboundMcpSettings,
  | 'enabled'
  | 'allowedOrigins'
  | 'maxRequestSize'
  | 'maxResponseSize'
  | 'maxConcurrentCalls'
  | 'callsPerMinute'
  | 'readTimeoutMs'
>;

export interface InboundMcpSettingsResponse {
  settings: InboundMcpSettings;
  endpoint: string;
  protocolVersions: string[];
  transports: string[];
  /** The most MiB a request or response size can be set to. */
  hardMaxSize: number;
  oauth: {
    protectedResourceMetadata: string;
    authorizationServerMetadata: string;
    authorizationEndpoint: string;
    tokenEndpoint: string;
    registrationEndpoint: string;
    revocationEndpoint: string;
  };
}

export interface InboundMcpOAuthConnection {
  familyId: string;
  clientId: string;
  clientName: string;
  userId: string;
  userDisplayName: string;
  scope: string;
  resource: string;
  createdAt: number;
  lastExpiresAt: number;
  active: boolean;
}

export async function getSettings(): Promise<InboundMcpSettingsResponse> {
  const { data } = await http.get<InboundMcpSettingsResponse>(
    '/agent/settings/inbound-mcp',
  );
  return data;
}

export async function updateSettings(
  input: InboundMcpSettingsInput,
): Promise<InboundMcpSettingsResponse> {
  const { data } = await http.put<InboundMcpSettingsResponse>(
    '/agent/settings/inbound-mcp',
    input,
  );
  return data;
}

export async function listOAuthConnections(): Promise<InboundMcpOAuthConnection[]> {
  const { data } = await http.get<{ connections: InboundMcpOAuthConnection[] }>(
    '/agent/settings/inbound-mcp/oauth-connections',
  );
  return data.connections ?? [];
}

export async function revokeOAuthConnection(familyId: string): Promise<void> {
  await http.post(
    `/agent/settings/inbound-mcp/oauth-connections/${encodeURIComponent(familyId)}/revoke`,
  );
}
