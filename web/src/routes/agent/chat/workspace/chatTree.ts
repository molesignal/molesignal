import type { Chat } from '@/api/agent/chat';

import { type ChatDateGroup, chatDateGroup } from '../presentation';

/** A chat in the list, and how deep it hangs under the chat it was forked from. */
export interface ChatRow {
  chat: Chat;
  depth: number;
}

export interface ChatListGroup {
  key: ChatDateGroup;
  rows: ChatRow[];
}

const DATE_ORDER: ChatDateGroup[] = ['today', 'yesterday', 'last_7_days', 'older'];

/** Forks hang at most this deep; deeper ones are shown at this depth. */
const MAX_DEPTH = 3;

/**
 * The chats as the list shows them, by day. A fork hangs under the chat it came
 * from, and a chat with its forks is dated by the latest of them, so a line of
 * work that is still going stays near the top.
 */
export function arrangeChats(chats: readonly Chat[], now = new Date()): ChatListGroup[] {
  const present = new Set(chats.map((chat) => chat.id));
  const forks = new Map<string, Chat[]>();
  for (const chat of chats) {
    const parent = chat.parent_chat_id;
    if (!parent || !present.has(parent)) continue;
    forks.set(parent, [...(forks.get(parent) ?? []), chat]);
  }

  const latest = new Map<string, number>();
  const latestOf = (chat: Chat): number => {
    const known = latest.get(chat.id);
    if (known !== undefined) return known;
    latest.set(chat.id, chat.updated_at_micros);
    const value = Math.max(chat.updated_at_micros, ...(forks.get(chat.id) ?? []).map(latestOf));
    latest.set(chat.id, value);
    return value;
  };
  const newestFirst = (a: Chat, b: Chat) => latestOf(b) - latestOf(a);

  const rowsOf = (chat: Chat, depth: number, seen: Set<string>): ChatRow[] => {
    if (seen.has(chat.id)) return [];
    seen.add(chat.id);
    const children = [...(forks.get(chat.id) ?? [])].sort(newestFirst);
    return [
      { chat, depth: Math.min(depth, MAX_DEPTH) },
      ...children.flatMap((child) => rowsOf(child, depth + 1, seen)),
    ];
  };

  const roots = chats
    .filter((chat) => !chat.parent_chat_id || !present.has(chat.parent_chat_id))
    .sort(newestFirst);
  const seen = new Set<string>();
  const groups: ChatListGroup[] = [];
  for (const key of DATE_ORDER) {
    const rows = roots
      .filter((chat) => chatDateGroup(latestOf(chat), now) === key)
      .flatMap((chat) => rowsOf(chat, 0, seen));
    if (rows.length > 0) groups.push({ key, rows });
  }
  return groups;
}
