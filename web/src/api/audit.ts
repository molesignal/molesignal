import { http } from '@/lib/http';

import type { ChatMessageRow, Chat } from './agent/chat';

export interface AuditEvent {
  id: string;
  orgId: string;
  actorKind: string;
  actorId: string;
  action: string;
  targetKind?: string | null;
  targetId?: string | null;
  ip?: string | null;
  userAgent?: string | null;
  payload: Record<string, unknown>;
  ts: number;
}

export interface AuditQueryParams {
  from?: string | undefined;
  to?: string | undefined;
  actorKind?: string | undefined;
  actor?: string | undefined;
  action?: string | undefined;
  targetKind?: string | undefined;
  targetId?: string | undefined;
  limit?: number | undefined;
  cursor?: string | undefined;
}

export interface AuditPage {
  items: AuditEvent[];
  nextCursor: string | null;
  /** 审计记录保留天数（部署级设置）；0 表示永久保留。 */
  retentionDays: number;
}

export interface AuditChat extends Chat {
  deletedAt?: number | null;
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

/** Admin/Owner audit view for a Mole Agent chat, including soft-deleted chats. */
export async function getAgentChatTranscript(id: string): Promise<AuditChatTranscript> {
  const { data } = await http.get<AuditChatTranscript>(
    `/agent/audit/chat/${encodeURIComponent(id)}`,
  );
  return data;
}
