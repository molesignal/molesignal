import type { ChatMessageRow } from '@/api/intelligence/chat';

export interface PendingUserMessage {
  chatId: string;
  content: string;
  sentAtMicros: number;
}

export interface ConversationTurn {
  kind: 'turn';
  user: ChatMessageRow;
  answers: ChatMessageRow[];
}

export interface StandaloneAssistantMessage {
  kind: 'assistant';
  message: ChatMessageRow;
}

export type ConversationItem = ConversationTurn | StandaloneAssistantMessage;

export function visibleChatMessages(messages: ChatMessageRow[]): ChatMessageRow[] {
  const visible: ChatMessageRow[] = [];
  for (const message of messages) {
    const previous = visible[visible.length - 1];
    if (previous && sameVisibleMessage(previous, message)) continue;
    visible.push(message);
  }
  return visible;
}

/**
 * Product-facing chat history groups consecutive assistant rows under the
 * preceding user question. A regeneration appends another assistant row but
 * does not append the user row, so it becomes an answer version instead of a
 * duplicated question.
 */
export function conversationItems(messages: ChatMessageRow[]): ConversationItem[] {
  const items: ConversationItem[] = [];
  for (const message of visibleChatMessages(messages)) {
    if (message.role === 'tool' || message.role === 'system') continue;
    if (message.role === 'user') {
      items.push({ kind: 'turn', user: message, answers: [] });
      continue;
    }
    if (message.role !== 'assistant') continue;
    const previous = items[items.length - 1];
    if (previous?.kind === 'turn') {
      previous.answers.push(message);
    } else {
      items.push({ kind: 'assistant', message });
    }
  }
  return items;
}

export function hasPersistedPendingUser(
  messages: ChatMessageRow[],
  pending: PendingUserMessage,
): boolean {
  const lastMessage = lastChatMessage(messages);
  if (!lastMessage) return false;
  const content = normalizeMessageContent(pending.content);
  return (
    lastMessage.role === 'user' &&
    lastMessage.chat_id === pending.chatId &&
    normalizeMessageContent(lastMessage.content) === content
  );
}

export function hasPersistedStreamError(messages: ChatMessageRow[], error: string): boolean {
  const expected = normalizeStreamError(error);
  return messages.some(
    (m) => m.role === 'assistant' && normalizeStreamError(m.content) === expected,
  );
}

export function formatStreamErrorContent(error: string): string {
  const trimmed = error.trim();
  return trimmed.startsWith('[error:') ? trimmed : `[error: ${trimmed}]`;
}

function normalizeStreamError(error: string): string {
  const trimmed = error.trim();
  const match = /^\[error:\s*([\s\S]*)\]$/.exec(trimmed);
  return (match?.[1] ?? trimmed).trim();
}

function lastChatMessage(messages: ChatMessageRow[]): ChatMessageRow | null {
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    if (messages[i]?.role !== 'tool') return messages[i] ?? null;
  }
  return null;
}

function sameVisibleMessage(a: ChatMessageRow, b: ChatMessageRow): boolean {
  return (
    a.chat_id === b.chat_id &&
    a.role === 'user' &&
    b.role === 'user' &&
    normalizeMessageContent(a.content) === normalizeMessageContent(b.content)
  );
}

function normalizeMessageContent(content: string): string {
  return content.trim().replace(/\s+/g, ' ');
}
