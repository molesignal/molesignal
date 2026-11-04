import type { TFunction } from 'i18next';
import { describe, expect, it } from 'vitest';

import { auditStatusTone, distinctValues, humanAuditAction, isAccessAction } from './humanAction';

const t = ((key: string, options?: { defaultValue?: string }) => {
  const known: Record<string, string> = {
    'audit.words.agent': 'Agent',
    'audit.words.chat': '对话',
    'audit.words.archived': '已归档',
  };
  return known[key] ?? options?.defaultValue ?? key;
}) as unknown as TFunction<'settings-admin'>;

describe('humanAuditAction', () => {
  it('reads a key word by word', () => {
    expect(humanAuditAction('agent.chat.archived', t)).toBe('Agent 对话 已归档');
  });

  it('leaves a word nobody translated as it is', () => {
    expect(humanAuditAction('agent.brand_new', t)).toBe('Agent brand new');
  });
});

describe('isAccessAction', () => {
  it('finds what touches who can get in', () => {
    for (const action of ['mfa.reset', 'signin.mfa.lockout', 'sessions.revoke_others', 'agent.automation.token_issued', 'alerts.source.token_rotated', 'resource_share.unlock']) {
      expect(isAccessAction(action), action).toBe(true);
    }
  });

  it('leaves out the rest', () => {
    for (const action of ['agent.chat.archived', 'stream_retention.update', 'schedules.imported']) {
      expect(isAccessAction(action), action).toBe(false);
    }
  });
});

describe('auditStatusTone', () => {
  it('colours what went well and what went wrong', () => {
    for (const status of ['ok', 'success', 'succeeded', 'approved', 'executed', 'Succeeded']) {
      expect(auditStatusTone(status), status).toBe('green');
    }

    for (const status of ['failed', 'failure', 'error', 'rejected', 'denied']) {
      expect(auditStatusTone(status), status).toBe('red');
    }
  });

  it('leaves a word that says neither way plain', () => {
    for (const status of ['pending', 'running', 'expired', 'cancelled', 'open', 'a new word']) {
      expect(auditStatusTone(status), status).toBe('neutral');
    }
  });
});

describe('distinctValues', () => {
  it('sorts what it saw, once each', () => {
    expect(distinctValues(['b', 'a', null, 'b', '', undefined])).toEqual(['a', 'b']);
  });
});
