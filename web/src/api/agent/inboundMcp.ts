import { http } from '@/lib/http';

export interface InboundMcpSettings {
  org_id: string;
  enabled: boolean;
  allowed_origins: string[];
  max_request_bytes: number;
  max_response_bytes: number;
  max_concurrent_calls: number;
  calls_per_minute: number;
  read_timeout_ms: number;
  updated_by: string;
  created_at: number;
  updated_at: number;
}

export type InboundMcpSettingsInput = Pick<
  InboundMcpSettings,
  | 'enabled'
  | 'allowed_origins'
  | 'max_request_bytes'
  | 'max_response_bytes'
  | 'max_concurrent_calls'
  | 'calls_per_minute'
  | 'read_timeout_ms'
>;

export interface InboundMcpSettingsResponse {
  settings: InboundMcpSettings;
  endpoint: string;
  protocol_versions: string[];
  transports: string[];
  hard_max_body_bytes: number;
  oauth: {
    protected_resource_metadata: string;
    authorization_server_metadata: string;
    authorization_endpoint: string;
    token_endpoint: string;
    registration_endpoint: string;
    revocation_endpoint: string;
  };
}

export interface InboundMcpOAuthConnection {
  family_id: string;
  client_id: string;
  client_name: string;
  user_id: string;
  user_display_name: string;
  scope: string;
  resource: string;
  created_at: number;
  last_expires_at: number;
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
