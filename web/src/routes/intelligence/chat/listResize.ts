export const CHAT_LIST_WIDTH_STORAGE_KEY = 'molesignal-intelligence-chat-list-width';
export const CHAT_LIST_DEFAULT_WIDTH = 240;
export const CHAT_LIST_MIN_WIDTH = 200;
export const CHAT_LIST_MAX_WIDTH = 420;
export const CHAT_LIST_RESIZE_STEP = 16;

export function clampChatListWidth(width: number): number {
  if (!Number.isFinite(width)) return CHAT_LIST_DEFAULT_WIDTH;
  return Math.min(CHAT_LIST_MAX_WIDTH, Math.max(CHAT_LIST_MIN_WIDTH, Math.round(width)));
}

export function parseStoredChatListWidth(value: string | null): number {
  if (!value?.trim()) return CHAT_LIST_DEFAULT_WIDTH;
  const width = Number(value);
  return Number.isFinite(width) ? clampChatListWidth(width) : CHAT_LIST_DEFAULT_WIDTH;
}

export function chatListWidthFromPointer(
  startWidth: number,
  startClientX: number,
  clientX: number,
): number {
  return clampChatListWidth(startWidth + clientX - startClientX);
}

export function chatListWidthFromKey(
  width: number,
  key: string,
  accelerated = false,
): number | null {
  const step = CHAT_LIST_RESIZE_STEP * (accelerated ? 2 : 1);
  if (key === 'ArrowLeft') return clampChatListWidth(width - step);
  if (key === 'ArrowRight') return clampChatListWidth(width + step);
  if (key === 'Home') return CHAT_LIST_MIN_WIDTH;
  if (key === 'End') return CHAT_LIST_MAX_WIDTH;
  return null;
}
