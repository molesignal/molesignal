import { http } from '@/lib/http';

/** Mirrors `molesignal_domain::query::QueryLanguage` (snake_case wire form). */
export type QueryLanguage = 'sql' | 'promql';

/** Mirrors `molesignal_domain::saved_view::SavedView`. */
export interface SavedView {
  id: string;
  org_id: string;
  owner_user_id: string;
  name: string;
  language: QueryLanguage;
  statement: string;
  /** Default look-back window, in seconds, applied when the view is opened. */
  time_range_secs: number;
  stream: string | null;
  tags: string[];
  pinned: boolean;
  /** Microseconds since epoch. */
  created_at: number;
  updated_at: number;
}

/** Request body for create / update (`WriteReq` on the backend). */
export interface SavedViewInput {
  name: string;
  language: QueryLanguage;
  statement: string;
  time_range_secs: number;
  stream?: string | null;
  tags?: string[];
  pinned?: boolean;
}

export async function list(pinnedOnly = false): Promise<SavedView[]> {
  const suffix = pinnedOnly ? '?pinned=true' : '';
  const { data } = await http.get<SavedView[]>(`/saved_views${suffix}`);
  return data;
}

export async function get(id: string): Promise<SavedView> {
  const { data } = await http.get<SavedView>(`/saved_views/${encodeURIComponent(id)}`);
  return data;
}

export async function create(input: SavedViewInput): Promise<SavedView> {
  const { data } = await http.post<SavedView>('/saved_views', input);
  return data;
}

export async function update(id: string, input: SavedViewInput): Promise<SavedView> {
  const { data } = await http.put<SavedView>(`/saved_views/${encodeURIComponent(id)}`, input);
  return data;
}

export async function remove(id: string): Promise<void> {
  await http.delete(`/saved_views/${encodeURIComponent(id)}`);
}
