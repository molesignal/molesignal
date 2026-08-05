import { describe, expect, it } from 'vitest';

import type { AgentPrompt } from '@/api/intelligence/prompts';

import {
  effectivePromptIds,
  parsePromptSchema,
  promptTemplateVariables,
  unknownPromptVariables,
} from './model';

describe('Mole Intelligence prompt models', () => {
  it('extracts template variables once and preserves their order', () => {
    expect(
      promptTemplateVariables(
        'Investigate {{ service }} in {{ time_range }} for {{service}}.',
      ),
    ).toEqual(['service', 'time_range']);
  });

  it('validates the variables schema and reports undeclared references', () => {
    const result = parsePromptSchema(
      JSON.stringify({
        type: 'object',
        properties: {
          service: { type: 'string' },
        },
      }),
    );
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(
      unknownPromptVariables(
        '{{ service }} / {{ environment }}',
        result.variables,
      ),
    ).toEqual(['environment']);
  });

  it('resolves effective defaults in user, organization, builtin order', () => {
    const base: AgentPrompt = {
      id: 'builtin',
      scope: 'builtin',
      purpose: 'system',
      name: 'Builtin',
      body: 'body',
      variables_schema: {},
      is_default: true,
      enabled: true,
      version: 1,
      created_at_micros: 1,
      updated_at_micros: 1,
    };
    const org = { ...base, id: 'org', scope: 'org' as const };
    const user = { ...base, id: 'user', scope: 'user' as const };
    expect(effectivePromptIds([base, org, user])).toEqual(new Set(['user']));
  });
});
