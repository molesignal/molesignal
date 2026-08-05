import { describe, expect, it } from 'vitest';

import type { ChatMessageRow } from '@/api/intelligence/chat';

import {
  conversationItems,
  formatStreamErrorContent,
  hasPersistedPendingUser,
  hasPersistedStreamError,
  type PendingUserMessage,
  visibleChatMessages,
} from './messageRenderState';

function message(overrides: Partial<ChatMessageRow>): ChatMessageRow {
  return {
    id: 'msg-1',
    chat_id: 'chat-1',
    org_id: 'default',
    role: 'assistant',
    content: '',
    created_at_micros: 1_000_000,
    ...overrides,
  };
}

describe('Mole Intelligence chat message render state', () => {
  it('hides the optimistic user bubble once the backend row arrives', () => {
    const pending: PendingUserMessage = {
      chatId: 'chat-1',
      content: '帮我写一个查询',
      sentAtMicros: 10_000_000,
    };

    expect(
      hasPersistedPendingUser(
        [
          message({
            role: 'user',
            content: '帮我写一个查询',
            created_at_micros: 10_200_000,
          }),
        ],
        pending,
      ),
    ).toBe(true);
  });

  it('keeps the optimistic user bubble when an older repeated prompt already has an answer', () => {
    const pending: PendingUserMessage = {
      chatId: 'chat-1',
      content: '帮我写一个查询',
      sentAtMicros: 10_000_000,
    };

    expect(
      hasPersistedPendingUser(
        [
          message({
            role: 'user',
            content: '帮我写一个查询',
            created_at_micros: 1_000_000,
          }),
          message({
            id: 'msg-2',
            role: 'assistant',
            content: '上一轮回答',
            created_at_micros: 2_000_000,
          }),
        ],
        pending,
      ),
    ).toBe(false);
  });

  it('recognizes a persisted assistant error bubble', () => {
    const error =
      'internal: openai status 400 Bad Request: {"error":{"message":"bad request"}}';

    expect(
      hasPersistedStreamError(
        [
          message({
            role: 'assistant',
            content: formatStreamErrorContent(error),
          }),
        ],
        error,
      ),
    ).toBe(true);
  });

  it('deduplicates adjacent duplicate rendered messages', () => {
    const first = message({
      id: 'msg-1',
      role: 'user',
      content: '帮我写一个查询',
      created_at_micros: 1_000_000,
    });
    const duplicate = message({
      id: 'msg-2',
      role: 'user',
      content: ' 帮我写一个查询 ',
      created_at_micros: 1_100_000,
    });
    const answered = message({
      id: 'msg-3',
      role: 'assistant',
      content: '回答',
      created_at_micros: 2_000_000,
    });

    expect(visibleChatMessages([first, duplicate, answered])).toEqual([first, answered]);
  });

  it('keeps regenerated answers under one user question', () => {
    const user = message({
      id: 'user-1',
      role: 'user',
      content: '谁正在负责生产环境值班？',
      created_at_micros: 1_000_000,
    });
    const first = message({
      id: 'answer-1',
      role: 'assistant',
      content: '第一版回答',
      created_at_micros: 2_000_000,
    });
    const regenerated = message({
      id: 'answer-2',
      role: 'assistant',
      content: '第二版回答',
      created_at_micros: 3_000_000,
    });

    expect(conversationItems([user, first, regenerated])).toEqual([
      {
        kind: 'turn',
        user,
        answers: [first, regenerated],
      },
    ]);
  });

  it('keeps identical consecutive assistant rows as answer versions', () => {
    const first = message({
      id: 'answer-1',
      role: 'assistant',
      content: '相同回答',
    });
    const regenerated = message({
      id: 'answer-2',
      role: 'assistant',
      content: '相同回答',
    });
    expect(visibleChatMessages([first, regenerated])).toEqual([first, regenerated]);
  });

  it('keeps repeated questions as separate turns when a new user row exists', () => {
    const firstUser = message({
      id: 'user-1',
      role: 'user',
      content: '检查错误率',
      created_at_micros: 1_000_000,
    });
    const firstAnswer = message({
      id: 'answer-1',
      role: 'assistant',
      content: '第一次回答',
      created_at_micros: 2_000_000,
    });
    const secondUser = message({
      id: 'user-2',
      role: 'user',
      content: '检查错误率',
      created_at_micros: 3_000_000,
    });
    const secondAnswer = message({
      id: 'answer-2',
      role: 'assistant',
      content: '第二次回答',
      created_at_micros: 4_000_000,
    });

    expect(conversationItems([firstUser, firstAnswer, secondUser, secondAnswer])).toHaveLength(2);
  });
});
