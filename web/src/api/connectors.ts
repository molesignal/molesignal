import { http } from '@/lib/http';

export interface Connector {
  id: string;
  name: string;
  kind: string;
  /** Secrets read as `***` here; sent back unchanged, a backend that lists `secret_keys` keeps them. */
  config_json: Record<string, unknown>;
  /**
   * The secrets in `config_json` that hold a value. A backend that sends this
   * keeps a stored secret when the mask comes back on update; one that does
   * not would save the mask itself.
   */
  secret_keys?: string[];
  enabled: boolean;
  last_run_at_micros?: number;
  created_at_micros: number;
  updated_at_micros: number;
}

export interface ConnectorInput {
  name: string;
  kind: string;
  config_json: Record<string, unknown>;
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
