import { http } from '@/lib/http';

export type ModelProviderKind = 'openai' | 'anthropic' | 'openai_compatible';

export interface ModelProvider {
  id: string;
  provider: ModelProviderKind;
  name: string;
  baseUrl?: string | null;
  defaultModel: string;
  enabled: boolean;
  /** How long a call may go without receiving any data before it fails. */
  timeoutMs: number;
  maxTokens?: number | null;
  /** ISO 4217: what the prices, and the costs they give, are in. */
  priceCurrency: string;
  /**
   * Per million input tokens; with a provider that caches, the price of input
   * not found in the cache. Input and output both unset: cost is not computed.
   */
  inputPricePerMtok?: number | null;
  /** Per million input tokens found in the cache; unset, they cost the input price. */
  cachedInputPricePerMtok?: number | null;
  /** Per million output tokens. */
  outputPricePerMtok?: number | null;
  /** masked last 4 chars of the API key; the plaintext key is never returned. */
  keyLast4?: string | null;
  keySet: boolean;
  createdAt: number;
  updatedAt: number;
}

export interface CreateProviderInput {
  provider: ModelProviderKind;
  name: string;
  baseUrl?: string | undefined;
  defaultModel: string;
  enabled?: boolean | undefined;
  timeoutMs?: number | undefined;
  maxTokens?: number | undefined;
  priceCurrency?: string | undefined;
  inputPricePerMtok?: number | null | undefined;
  cachedInputPricePerMtok?: number | null | undefined;
  outputPricePerMtok?: number | null | undefined;
  /** write-only API key; stored encrypted, never echoed back. */
  apiKey?: string | undefined;
}

export interface UpdateProviderInput {
  provider: ModelProviderKind;
  name: string;
  baseUrl?: string | undefined;
  defaultModel: string;
  enabled: boolean;
  timeoutMs?: number | undefined;
  maxTokens?: number | undefined;
  priceCurrency?: string | undefined;
  inputPricePerMtok?: number | null | undefined;
  cachedInputPricePerMtok?: number | null | undefined;
  outputPricePerMtok?: number | null | undefined;
}

/** Default and bounds of `timeout_ms`, the same as the backend's. */
export const DEFAULT_TIMEOUT_MS = 120_000;
export const MIN_TIMEOUT_MS = 1_000;
export const MAX_TIMEOUT_MS = 600_000;

export async function list(): Promise<ModelProvider[]> {
  const { data } = await http.get<ModelProvider[]>('/agent/settings/model-providers');
  return data;
}

export async function create(input: CreateProviderInput): Promise<ModelProvider> {
  const { data } = await http.post<ModelProvider>('/agent/settings/model-providers', input);
  return data;
}

export async function update(id: string, input: UpdateProviderInput): Promise<ModelProvider> {
  const { data } = await http.put<ModelProvider>(
    `/agent/settings/model-providers/${encodeURIComponent(id)}`,
    input,
  );
  return data;
}

export async function rotateKey(id: string, apiKey: string): Promise<ModelProvider> {
  const { data } = await http.post<ModelProvider>(
    `/agent/settings/model-providers/${encodeURIComponent(id)}/rotate-key`,
    { apiKey: apiKey },
  );
  return data;
}

export async function remove(id: string): Promise<void> {
  await http.delete(`/agent/settings/model-providers/${encodeURIComponent(id)}`);
}
