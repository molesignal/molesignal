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
        defaultModel: 'gpt-5',
        enabled: true,
        timeoutMs: 30_000,
        priceCurrency: 'USD',
        keySet: true,
        createdAt: 1,
        updatedAt: 1,
      },
      {
        id: 'provider-internal',
        provider: 'openai_compatible',
        name: 'Internal Qwen',
        defaultModel: 'qwen3',
        enabled: false,
        timeoutMs: 30_000,
        priceCurrency: 'USD',
        keySet: false,
        createdAt: 2,
        updatedAt: 2,
      },
    ];

    expect(profileModelProviderOptions(providers, 'Automatic')).toEqual([
      { value: '', label: 'Automatic' },
      { value: 'provider-openai', label: 'OpenAI Production' },
      { value: 'provider-internal', label: 'Internal Qwen' },
    ]);
  });
});
