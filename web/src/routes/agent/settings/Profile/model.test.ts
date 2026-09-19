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
      { name: 'query_logs', available_to_agent: true },
      { name: 'execute_agent_approval', available_to_agent: false },
    ];

    expect(toolsAvailableToAgentProfiles(tools)).toEqual([tools[0]]);
  });

  it('removes unavailable tools from an existing profile draft', () => {
    const profile = {
      id: 'profile-1',
      name: 'Legacy profile',
      description: '',
      allowed_tools: ['query_logs', 'execute_agent_approval'],
      data_scope: {},
      risk_policy: {},
      network_access: 'blocked' as const,
      max_context_tokens: 32000,
      max_investigation_secs: 1800,
      max_tool_calls: 32,
      is_default: true,
      enabled: true,
      created_by: 'user-1',
      created_at: 1,
      updated_at: 1,
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
      model_provider_id: 'provider-1',
      model: 'gpt-5',
      allowed_tools: ['query_logs'],
      data_scope: {
        environments: ['production'],
        services: ['checkout', 'payments'],
        streams: ['application-logs'],
      },
      network_access: 'allowed',
      risk_policy: { l2: 'two_person_approval' },
      max_investigation_secs: 720,
      max_tool_calls: 7,
    });
  });
});
