import { describe, expect, it } from 'vitest';

import type { ImBot } from '@/api/agent/im';

import { botState, imDocumentationUrl, stateTone, teamChoice, teamOfChoice } from './model';

function bot(patch: Partial<ImBot> = {}): ImBot {
  return {
    connectorId: 'c1',
    connectorName: 'Ops bot',
    connectorType: 'lark_app',
    connectorEnabled: true,
    platform: 'feishu',
    missing: [],
    callbackUrl: null,
    enabled: true,
    defaultTeamId: null,
    defaultTeamName: null,
    status: 'connected',
    statusDetail: 'Mole · node-1',
    statusAt: 1,
    lastMessageAt: null,
    ...patch,
  };
}

describe('an IM bot row', () => {
  it('says what keeps it from answering before how it is connected', () => {
    expect(botState(bot({ missing: ['appToken'], enabled: false }))).toEqual({
      kind: 'missing',
      keys: ['appToken'],
    });
    expect(botState(bot({ connectorEnabled: false })).kind).toBe('connector_off');
    expect(botState(bot({ enabled: false })).kind).toBe('off');
    expect(botState(bot({ status: 'error', statusDetail: 'refused' }))).toEqual({
      kind: 'error',
      detail: 'refused',
    });
  });

  it('is green when connected and red when it cannot answer', () => {
    expect(stateTone(botState(bot()))).toBe('good');
    expect(stateTone(botState(bot({ status: 'error' })))).toBe('bad');
    expect(stateTone(botState(bot({ enabled: false })))).toBe('muted');
    expect(stateTone(botState(bot({ status: 'connecting' })))).toBe('muted');
  });

  it('reads the default team as one select value', () => {
    expect(teamChoice(bot())).toBe('personal');
    expect(teamChoice(bot({ defaultTeamId: 't1' }))).toBe('t1');
    expect(teamOfChoice('personal')).toBeNull();
    expect(teamOfChoice('t1')).toBe('t1');
  });

  it('links to the setup docs in the reader’s language', () => {
    expect(imDocumentationUrl('zh-cn')).toBe('https://docs.molesignal.com/zh-Hans/im');
    expect(imDocumentationUrl('en-us')).toBe('https://docs.molesignal.com/en-US/im');
  });
});
