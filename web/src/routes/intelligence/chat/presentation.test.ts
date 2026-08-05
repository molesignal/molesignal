import { describe, expect, it } from 'vitest';

import type { Chat } from '@/api/intelligence/chat';

import {
  chatDateGroup,
  displayTitleForChat,
  groupChats,
  titleForNewChat,
} from './presentation';

function chat(id: string, title: string, updatedAt: string): Chat {
  const micros = Date.parse(updatedAt) * 1000;
  return {
    id,
    provider: 'openai',
    model: 'gpt-5',
    title,
    created_at_micros: micros,
    updated_at_micros: micros,
  };
}

describe('Mole Intelligence chat presentation', () => {
  const now = new Date('2026-07-26T15:30:00+08:00');

  it('groups chats by local calendar day', () => {
    expect(
      chatDateGroup(Date.parse('2026-07-26T09:00:00+08:00') * 1000, now),
    ).toBe('today');
    expect(
      chatDateGroup(Date.parse('2026-07-25T09:00:00+08:00') * 1000, now),
    ).toBe('yesterday');
    expect(
      groupChats(
        [
          chat('today', 'Today', '2026-07-26T09:00:00+08:00'),
          chat('older', 'Older', '2026-06-01T09:00:00+08:00'),
        ],
        now,
      ).map((group) => group.key),
    ).toEqual(['today', 'older']);
  });

  it('adds a time suffix when a generated title would repeat', () => {
    const existing = [
      chat(
        'existing',
        'Why is checkout failing?',
        '2026-07-26T09:00:00+08:00',
      ),
    ];
    expect(titleForNewChat('Why is checkout failing?', existing, now)).toBe(
      'Why is checkout failing? · 15:30',
    );
  });

  it('disambiguates existing duplicate titles in the sidebar', () => {
    const first = chat(
      'first',
      'checkout-api 错误率升高',
      '2026-07-26T13:17:00',
    );
    const second = chat(
      'second',
      'checkout-api 错误率升高',
      '2026-07-26T14:20:00',
    );
    expect(displayTitleForChat(first, [first, second])).toBe(
      'checkout-api 错误率升高 · 13:17',
    );
    expect(displayTitleForChat(second, [first, second])).toBe(
      'checkout-api 错误率升高 · 14:20',
    );
  });
});
