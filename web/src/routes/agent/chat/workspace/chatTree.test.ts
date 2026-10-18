import { describe, expect, it } from 'vitest';

import type { Chat } from '@/api/agent/chat';

import { arrangeChats } from './chatTree';

const NOW = new Date(2026, 9, 17, 12, 0);
const DAY = 86_400_000_000;
const at = (daysAgo: number, minutes = 0) => NOW.getTime() * 1000 - daysAgo * DAY + minutes * 60_000_000;

function chat(id: string, updated: number, parent: string | null = null): Chat {
  return {
    id,
    provider: 'openai',
    model: 'gpt',
    title: id,
    parent_chat_id: parent,
    created_at_micros: updated,
    updated_at_micros: updated,
  } as Chat;
}

describe('the chat list', () => {
  it('hangs a fork under the chat it came from, dated by the latest of them', () => {
    const groups = arrangeChats(
      [chat('rum', at(3)), chat('geoip', at(0, -5), 'rum'), chat('other', at(0, -30))],
      NOW,
    );
    expect(groups.map((group) => group.key)).toEqual(['today']);
    expect(groups[0]?.rows.map((row) => [row.chat.id, row.depth])).toEqual([
      ['rum', 0],
      ['geoip', 1],
      ['other', 0],
    ]);
  });

  it('shows a fork on its own when its parent is gone', () => {
    const groups = arrangeChats(
      [chat('orphan', at(0), 'deleted'), chat('p', at(1)), chat('f', at(1), 'p'), chat('ff', at(1), 'f')],
      NOW,
    );
    expect(groups.flatMap((group) => group.rows.map((row) => [row.chat.id, row.depth]))).toEqual([
      ['orphan', 0],
      ['p', 0],
      ['f', 1],
      ['ff', 2],
    ]);
  });
});
