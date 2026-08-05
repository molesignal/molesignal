import { http } from '@/lib/http';

export type ModelProviderKind = 'openai' | 'anthropic' | 'openai_compatible';

export interface ModelProvider {
  id: string;
  provider: ModelProviderKind;
  name: string;
  base_url?: string | null;
  default_model: string;
  enabled: boolean;
  timeout_ms: number;
  max_tokens?: number | null;
  /** masked last 4 chars of the API key; the plaintext key is never returned. */
  key_last4?: string | null;
  key_set: boolean;
  created_at_micros: number;
  updated_at_micros: number;
}

export interface CreateProviderInput {
  provider: ModelProviderKind;
  name: string;
  base_url?: string | undefined;
  default_model: string;
  enabled?: boolean | undefined;
  timeout_ms?: number | undefined;
  max_tokens?: number | undefined;
  /** write-only API key; stored encrypted, never echoed back. */
  api_key?: string | undefined;
}

export interface UpdateProviderInput {
  provider: ModelProviderKind;
  name: string;
  base_url?: string | undefined;
  default_model: string;
  enabled: boolean;
  timeout_ms?: number | undefined;
  max_tokens?: number | undefined;
}

export async function list(): Promise<ModelProvider[]> {
  const { data } = await http.get<ModelProvider[]>('/intelligence/settings/model-providers');
  return data;
}

export async function create(input: CreateProviderInput): Promise<ModelProvider> {
  const { data } = await http.post<ModelProvider>('/intelligence/settings/model-providers', input);
  return data;
}

export async function update(id: string, input: UpdateProviderInput): Promise<ModelProvider> {
  const { data } = await http.put<ModelProvider>(
    `/intelligence/settings/model-providers/${encodeURIComponent(id)}`,
    input,
  );
  return data;
}

export async function rotateKey(id: string, apiKey: string): Promise<ModelProvider> {
  const { data } = await http.post<ModelProvider>(
    `/intelligence/settings/model-providers/${encodeURIComponent(id)}/rotate-key`,
    { api_key: apiKey },
  );
  return data;
}

export async function remove(id: string): Promise<void> {
  await http.delete(`/intelligence/settings/model-providers/${encodeURIComponent(id)}`);
}
