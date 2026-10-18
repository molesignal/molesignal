import { useUiPreference } from '@/shell/uiPreference/useUiPreference';

interface MessagePins {
  messages: string[];
}

/** The most answers one keeps pinned in a chat, the same as the server's. */
const MAX_PINS = 100;

function parsePins(raw: unknown): MessagePins | null {
  if (!raw || typeof raw !== 'object') return null;
  const messages = (raw as { messages?: unknown }).messages;
  return Array.isArray(messages) && messages.every((id) => typeof id === 'string')
    ? { messages }
    : null;
}

/** The answers a person keeps at the top of a chat: their own, kept as a preference. */
export function useMessagePins(chatId: string | null) {
  const pins = useUiPreference('agent.message_pins', chatId, parsePins);
  const pinnedIds = pins.value?.messages ?? [];
  const toggle = (messageId: string) => {
    const next = pinnedIds.includes(messageId)
      ? pinnedIds.filter((id) => id !== messageId)
      : [messageId, ...pinnedIds];
    pins.save({ messages: next.slice(0, MAX_PINS) });
  };
  return { pinnedIds, toggle };
}
