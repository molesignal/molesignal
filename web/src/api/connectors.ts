import { http } from '@/lib/http';

export interface ConnectorReadNote {
  atTime: number;
  text: string;
}

export interface ConnectorReadState {
  lastError?: ConnectorReadNote | null;
  lastWarning?: ConnectorReadNote | null;
}

export interface Connector {
  id: string;
  name: string;
  kind: string;
  /** Secrets read as `***` here; sent back unchanged, a backend that lists `secret_keys` keeps them. */
  configJson: Record<string, unknown>;
  /**
   * The secrets in `config_json` that hold a value. A backend that sends this
   * keeps a stored secret when the mask comes back on update; one that does
   * not would save the mask itself.
   */
  secretKeys?: string[];
  enabled: boolean;
  lastRunAt?: number;
  /** How a source's reading goes: where it has got to, and the latest failure and skip. */
  readState?: ConnectorReadState | null;
  createdAt: number;
  updatedAt: number;
}

export interface ConnectorInput {
  name: string;
  kind: string;
  configJson: Record<string, unknown>;
  enabled?: boolean;
}

export async function list(): Promise<Connector[]> {
  const { data } = await http.get<Connector[]>('/connectors');
  return data;
}

export async function get(id: string): Promise<Connector> {
  const { data } = await http.get<Connector>(`/connectors/${encodeURIComponent(id)}`);
  return data;
}

export async function create(payload: ConnectorInput): Promise<Connector> {
  const { data } = await http.post<Connector>('/connectors', {
    ...payload,
    enabled: payload.enabled ?? true,
  });
  return data;
}

export async function update(id: string, payload: ConnectorInput): Promise<Connector> {
  const { data } = await http.put<Connector>(`/connectors/${encodeURIComponent(id)}`, {
    ...payload,
    enabled: payload.enabled ?? true,
  });
  return data;
}

export async function remove(id: string): Promise<void> {
  await http.delete(`/connectors/${encodeURIComponent(id)}`);
}

/** What went wrong in a connection test, worded by the interface; `ok` when nothing did. */
export type ConnectionCheckCode =
  | 'ok'
  | 'invalid'
  | 'no_credentials'
  | 'denied'
  | 'not_found'
  | 'tls'
  | 'unreachable'
  | 'timeout'
  | 'failed';

export interface ConnectionCheck {
  ok: boolean;
  code: ConnectionCheckCode;
  /** What the other side or the client said, in its own words. */
  detail?: string | null;
  elapsedMs: number;
}

export interface ConnectionTestInput {
  kind: string;
  configJson: Record<string, unknown>;
  /** The saved connector whose stored secrets a mask stands for. */
  id?: string;
}

/** Tries settings before they are saved. */
export async function testConnection(input: ConnectionTestInput): Promise<ConnectionCheck> {
  const { data } = await http.post<ConnectionCheck>('/connectors/test', input);
  return data;
}

/** The external ID this organization assumes AWS roles with. */
export async function awsExternalId(): Promise<string> {
  const { data } = await http.get<{ externalId: string }>('/connectors/aws-external-id');
  return data.externalId;
}
