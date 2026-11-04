import { describe, expect, it } from 'vitest';

import {
  createProfileDraft,
  profileEditorPanes,
  profileInputFromDraft,
  toolsAvailableToAgentProfiles,
} from './model';

describe('Mole Agent profile tool selection', () => {
  it('excludes tools that are not exposed to Mole Agent', () => {
    const tools = [
      { name: 'query_logs', availableToAgent: true },
      { name: 'execute_agent_approval', availableToAgent: false },
    ];

    expect(toolsAvailableToAgentProfiles(tools)).toEqual([tools[0]]);
  });

  it('removes unavailable tools from an existing profile draft', () => {
    const profile = {
      id: 'profile-1',
      name: 'Legacy profile',
      description: '',
      allowedTools: ['query_logs', 'execute_agent_approval'],
      dataScope: {},
      riskPolicy: {},
      networkAccess: 'blocked' as const,
      maxContextTokens: 32000,
      maxInvestigationSecs: 1800,
      maxToolCalls: 32,
      isDefault: true,
      enabled: true,
      createdBy: 'user-1',
      createdAt: 1,
      updatedAt: 1,
    };

    expect(createProfileDraft(profile, 1, ['query_logs']).allowedTools).toEqual([
      'query_logs',
    ]);
  });

  it('exposes every independent constraint in the full profile editor', () => {
    expect(profileEditorPanes('profile')).toEqual([
      'identity',
      'tools',
      'data',
      'network',
      'limits',
      'approvals',
    ]);
    expect(profileEditorPanes('tools')).toEqual(['tools']);
  });

  it('builds a profile input with its own tools, scope, network, and policy', () => {
    const draft = {
      ...createProfileDraft('new', 1, ['query_logs']),
      name: 'Production read-only',
      providerId: 'provider-1',
      model: 'gpt-5',
      environments: ['production'],
      services: ['checkout', 'payments'],
      streams: ['application-logs'],
      networkAccess: 'allowed' as const,
      maxInvestigationMinutes: '12',
      maxToolCalls: '7',
      l2Policy: 'two_person_approval',
    };

    expect(profileInputFromDraft(draft, 'Fallback')).toMatchObject({
      name: 'Production read-only',
      modelProviderId: 'provider-1',
      model: 'gpt-5',
      allowedTools: ['query_logs'],
      dataScope: {
        environments: ['production'],
        services: ['checkout', 'payments'],
        streams: ['application-logs'],
      },
      networkAccess: 'allowed',
      riskPolicy: { l2: 'two_person_approval' },
      maxInvestigationSecs: 720,
      maxToolCalls: 7,
    });
  });

  it('keeps the default temperature unless one is given, and bounds the run limits', () => {
    const base = { ...createProfileDraft('new', 1, ['query_logs']), name: 'Bounded' };

    expect(profileInputFromDraft(base, 'Fallback')?.temperature).toBeNull();
    expect(
      profileInputFromDraft({ ...base, temperature: '0.7' }, 'Fallback')?.temperature,
    ).toBe(0.7);
    expect(profileInputFromDraft({ ...base, temperature: '2.5' }, 'Fallback')).toBeNull();
    expect(profileInputFromDraft({ ...base, temperature: '-1' }, 'Fallback')).toBeNull();
    expect(
      profileInputFromDraft({ ...base, maxInvestigationMinutes: '121' }, 'Fallback'),
    ).toBeNull();
  });
});
