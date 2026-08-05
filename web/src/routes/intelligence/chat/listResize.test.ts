import { describe, expect, it } from 'vitest';

import {
  CHAT_LIST_DEFAULT_WIDTH,
  CHAT_LIST_MAX_WIDTH,
  CHAT_LIST_MIN_WIDTH,
  chatListWidthFromKey,
  chatListWidthFromPointer,
  clampChatListWidth,
  parseStoredChatListWidth,
} from './listResize';

describe('Mole Intelligence chat list resizing', () => {
  it('clamps pointer resizing to the supported width range', () => {
    expect(chatListWidthFromPointer(240, 400, 460)).toBe(300);
    expect(chatListWidthFromPointer(240, 400, 100)).toBe(CHAT_LIST_MIN_WIDTH);
    expect(chatListWidthFromPointer(240, 400, 900)).toBe(CHAT_LIST_MAX_WIDTH);
  });

  it('supports keyboard resizing and accelerated steps', () => {
    expect(chatListWidthFromKey(240, 'ArrowLeft')).toBe(224);
    expect(chatListWidthFromKey(240, 'ArrowRight', true)).toBe(272);
    expect(chatListWidthFromKey(240, 'Home')).toBe(CHAT_LIST_MIN_WIDTH);
    expect(chatListWidthFromKey(240, 'End')).toBe(CHAT_LIST_MAX_WIDTH);
    expect(chatListWidthFromKey(240, 'Enter')).toBeNull();
  });

  it('restores valid stored widths and rejects invalid values', () => {
    expect(parseStoredChatListWidth('312')).toBe(312);
    expect(parseStoredChatListWidth('999')).toBe(CHAT_LIST_MAX_WIDTH);
    expect(parseStoredChatListWidth(null)).toBe(CHAT_LIST_DEFAULT_WIDTH);
    expect(parseStoredChatListWidth('not-a-number')).toBe(CHAT_LIST_DEFAULT_WIDTH);
    expect(clampChatListWidth(Number.NaN)).toBe(CHAT_LIST_DEFAULT_WIDTH);
  });
});
