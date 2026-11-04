import { http } from '@/lib/http';

export type FunctionLanguage = 'vrl' | 'js';

export interface FunctionResp {
  id: string;
  name: string;
  language: FunctionLanguage;
  source: string;
  paramsSchema: Record<string, unknown>;
  /** Built-in preset (seeded under the `__builtin__` org): read-only, shown with a badge. */
  isBuiltin: boolean;
  /** Human description (built-in presets carry one; user functions are usually empty). */
  description: string;
  createdAt: number;
  updatedAt: number;
}

export interface FunctionInput {
  name: string;
  language: FunctionLanguage;
  source: string;
  paramsSchema?: Record<string, unknown>;
}

export interface FunctionRunInput {
  language: FunctionLanguage;
  source: string;
  input: unknown;
  /** What the function reads beside the event (VRL `%name`), such as the stream it was read from. */
  metadata?: Record<string, unknown>;
}

export interface FunctionRunResp {
  output: unknown;
  /** The function dropped the event on purpose (VRL `abort`): it would go no further. */
  dropped?: boolean;
}

export async function list(): Promise<FunctionResp[]> {
  const { data } = await http.get<FunctionResp[]>('/functions');
  return data;
}

export async function get(id: string): Promise<FunctionResp> {
  const { data } = await http.get<FunctionResp>(`/functions/${encodeURIComponent(id)}`);
  return data;
}

export async function create(payload: FunctionInput): Promise<FunctionResp> {
  const { data } = await http.post<FunctionResp>('/functions', {
    ...payload,
    paramsSchema: payload.paramsSchema ?? {},
  });
  return data;
}

export async function update(id: string, payload: FunctionInput): Promise<FunctionResp> {
  const { data } = await http.put<FunctionResp>(`/functions/${encodeURIComponent(id)}`, {
    ...payload,
    paramsSchema: payload.paramsSchema ?? {},
  });
  return data;
}

export async function remove(id: string): Promise<void> {
  await http.delete(`/functions/${encodeURIComponent(id)}`);
}

export async function run(input: FunctionRunInput): Promise<FunctionRunResp> {
  const { data } = await http.post<FunctionRunResp>('/functions/run', input);
  return data;
}
