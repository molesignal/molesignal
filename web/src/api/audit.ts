import { http } from '@/lib/http';

import type { ChatMessageRow, Chat } from './intelligence/chat';

export interface AuditEvent {
  id: string;
  org_id: string;
  actor_kind: string;
  actor_id: string;
  action: string;
  target_kind?: string | null;
  target_id?: string | null;
  ip?: string | null;
  user_agent?: string | null;
  payload: Record<string, unknown>;
  ts_micros: number;
}

export interface AuditQueryParams {
  from?: string | undefined;
  to?: string | undefined;
  actor_kind?: string | undefined;
  actor?: string | undefined;
  action?: string | undefined;
  target_kind?: string | undefined;
  target_id?: string | undefined;
  limit?: number | undefined;
  cursor?: string | undefined;
}

export interface AuditPage {
  items: AuditEvent[];
  next_cursor: string | null;
}

export interface AuditChat extends Chat {
  deleted_at_micros?: number | null;
}

export interface AuditChatTranscript {
  chat: AuditChat;
  messages: ChatMessageRow[];
}

/** Filtered, cursor-paginated audit query (Admin+). */
export async function query(params: AuditQueryParams = {}): Promise<AuditPage> {
  // Drop empty params so we don't send `?action=` etc.
  const clean: Record<string, string | number> = {};
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined && v !== null && v !== '') clean[k] = v as string | number;
  }
  const { data } = await http.get<AuditPage>('/audit', { params: clean });
  return data;
}

/** Backwards-compatible recent-events helper used by activity widgets. */
export async function recent(limit = 10): Promise<AuditEvent[]> {
  const page = await query({ limit });
  return page.items;
}

/** Admin/Owner audit view for a Mole Intelligence chat, including soft-deleted chats. */
export async function getIntelligenceChatTranscript(id: string): Promise<AuditChatTranscript> {
  const { data } = await http.get<AuditChatTranscript>(
    `/intelligence/audit/chat/${encodeURIComponent(id)}`,
  );
  return data;
}
