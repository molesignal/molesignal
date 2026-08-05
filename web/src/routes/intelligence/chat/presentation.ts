import type { Chat } from '@/api/intelligence/chat';

export type ChatDateGroup = 'today' | 'yesterday' | 'last_7_days' | 'older';

export function chatDateGroup(
  updatedAtMicros: number,
  now = new Date(),
): ChatDateGroup {
  const timestamp = new Date(updatedAtMicros / 1000);
  const today = startOfDay(now);
  const yesterday = new Date(today);
  yesterday.setDate(yesterday.getDate() - 1);
  const lastWeek = new Date(today);
  lastWeek.setDate(lastWeek.getDate() - 7);

  if (timestamp >= today) return 'today';
  if (timestamp >= yesterday) return 'yesterday';
  if (timestamp >= lastWeek) return 'last_7_days';
  return 'older';
}

export function groupChats(
  chats: Chat[],
  now = new Date(),
): Array<{ key: ChatDateGroup; chats: Chat[] }> {
  const order: ChatDateGroup[] = [
    'today',
    'yesterday',
    'last_7_days',
    'older',
  ];
  return order
    .map((key) => ({
      key,
      chats: chats.filter(
        (chat) => chatDateGroup(chat.updated_at_micros, now) === key,
      ),
    }))
    .filter((group) => group.chats.length > 0);
}

export function titleForNewChat(
  content: string,
  chats: Chat[],
  now = new Date(),
): string {
  const base = content.trim().slice(0, 60);
  const duplicate = chats.some(
    (chat) => chat.title.trim().toLocaleLowerCase() === base.toLocaleLowerCase(),
  );
  if (!duplicate) return base;
  const hh = String(now.getHours()).padStart(2, '0');
  const mm = String(now.getMinutes()).padStart(2, '0');
  return `${base} · ${hh}:${mm}`;
}

export function displayTitleForChat(chat: Chat, chats: Chat[]): string {
  const title = chat.title.trim();
  if (!title) return '';
  const duplicateCount = chats.filter(
    (candidate) =>
      candidate.title.trim().toLocaleLowerCase() === title.toLocaleLowerCase(),
  ).length;
  if (duplicateCount <= 1 || / · \d{2}:\d{2}$/.test(title)) return title;
  const updated = new Date(chat.updated_at_micros / 1000);
  const hh = String(updated.getHours()).padStart(2, '0');
  const mm = String(updated.getMinutes()).padStart(2, '0');
  return `${title} · ${hh}:${mm}`;
}

function startOfDay(value: Date): Date {
  const start = new Date(value);
  start.setHours(0, 0, 0, 0);
  return start;
}
