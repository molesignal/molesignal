import { describe, expect, it } from 'vitest';

import type { ModelProvider } from '@/api/agent/modelProviders';

import { profileModelProviderOptions } from './EditorFields';

describe('profileModelProviderOptions', () => {
  it('includes automatic selection followed by every created model provider', () => {
    const providers: ModelProvider[] = [
      {
        id: 'provider-openai',
        provider: 'openai',
        name: 'OpenAI Production',
        default_model: 'gpt-5',
        enabled: true,
        timeout_ms: 30_000,
        key_set: true,
        created_at_micros: 1,
        updated_at_micros: 1,
      },
      {
        id: 'provider-internal',
        provider: 'openai_compatible',
        name: 'Internal Qwen',
        default_model: 'qwen3',
        enabled: false,
        timeout_ms: 30_000,
        key_set: false,
        created_at_micros: 2,
        updated_at_micros: 2,
      },
    ];

    expect(profileModelProviderOptions(providers, 'Automatic')).toEqual([
      { value: '', label: 'Automatic' },
      { value: 'provider-openai', label: 'OpenAI Production' },
      { value: 'provider-internal', label: 'Internal Qwen' },
    ]);
  });
});
