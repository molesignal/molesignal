import { http } from '@/lib/http';
import type { Dashboard } from '@/types/dashboard';

type DashboardListResponse = Dashboard[] | { items?: unknown[] };

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object';
}

function stringValue(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() ? value : undefined;
}

function numberValue(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined;
}

function stringArray(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : [];
}

function normalizeDashboard(raw: unknown): Dashboard {
  const record = isRecord(raw) ? raw : {};
  const modelRecord = isRecord(record.model) ? record.model : {};
  const title = stringValue(record.title) ?? 'Untitled dashboard';
  const id = stringValue(record.id) ?? '';
  const folderId = stringValue(record.folder_id);
  const normalized: Dashboard = {
    id,
    org_id: stringValue(record.org_id) ?? '',
    uid: stringValue(record.uid) ?? '',
    title,
    tags: stringArray(record.tags),
    model: modelRecord,
    version: numberValue(record.version) ?? 1,
    created_at: numberValue(record.created_at) ?? 0,
    updated_at: numberValue(record.updated_at) ?? 0,
  };
  if (folderId) normalized.folder_id = folderId;
  const createdBy = stringValue(record.created_by);
  const updatedBy = stringValue(record.updated_by);
  if (createdBy) normalized.created_by = createdBy;
  if (updatedBy) normalized.updated_by = updatedBy;
  return normalized;
}

export async function list(): Promise<Dashboard[]> {
  const { data } = await http.get<DashboardListResponse>('/dashboards');
  const items = Array.isArray(data) ? data : isRecord(data) && Array.isArray(data.items) ? data.items : [];
  return items.map(normalizeDashboard);
}

export async function get(id: string): Promise<Dashboard> {
  const { data } = await http.get<Dashboard>(`/dashboards/${id}`);
  return normalizeDashboard(data);
}

export async function update(
  id: string,
  model: Record<string, unknown>,
  folderId?: string,
): Promise<Dashboard> {
  const { data } = await http.put<Dashboard>(`/dashboards/${id}`, {
    model,
    folder_id: folderId,
  });
  return normalizeDashboard(data);
}

export async function remove(id: string): Promise<void> {
  await http.delete(`/dashboards/${id}`);
}

export async function create(
  model: Record<string, unknown>,
  folderId?: string,
): Promise<Dashboard> {
  const { data } = await http.post<Dashboard>('/dashboards', {
    model,
    folder_id: folderId,
  });
  return normalizeDashboard(data);
}

/* ─────────── Variable resolve ─────────── */

export interface VariableQuery {
  name: string;
  query: string;
  /** Defaults to `query`. `sql` requires `stream` to be set. */
  kind?: 'query' | 'sql';
  /** Stream hint — required when `kind === 'sql'`, optional otherwise. */
  stream?: { name: string; stream_type?: string };
}

export interface VariableResolveRequest {
  variable: VariableQuery;
  time_range: { start: number; end: number };
  limit?: number;
}

export interface VariableResolveResponse {
  variable: VariableQuery;
  values: string[];
  default?: string;
}

/**
 * Resolves a dashboard template variable against the backend.
 * Backend route: `crates/api/src/http/routes/dashboard_variables.rs`.
 *
 * - `kind === 'query'`: backend translates `label_values(<metric>, <label>)`
 *   to `SELECT DISTINCT <label> FROM <metric>`. The backend infers the
 *   stream from the metric name; the request may leave `stream` empty.
 * - `kind === 'sql'`: passes the SQL through verbatim; `stream` must be
 *   present so the backend knows which table to plan against.
 *
 * Backend enforces a strict identifier whitelist
 * (`[A-Za-z_][A-Za-z0-9_:.-]*`) on metric and label tokens — callers do
 * not need to escape, but should sanitize obviously hostile input before
 * round-tripping.
 */
export async function resolveVariable(
  payload: VariableResolveRequest,
): Promise<VariableResolveResponse> {
  const { data } = await http.post<VariableResolveResponse>(
    '/dashboards/variables/resolve',
    payload,
  );
  return data;
}
