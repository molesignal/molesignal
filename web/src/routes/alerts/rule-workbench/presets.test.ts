import { describe, expect, it } from 'vitest';

import { PRESETS } from './presets';

describe('sample alert templates', () => {
  it('offers three templates bound to sample streams without changing the existing defaults', () => {
    const samples = Object.values(PRESETS).flat().filter((preset) => preset.id.startsWith('sample-'));
    expect(samples).toHaveLength(3);
    for (const preset of samples) {
      expect(preset.statement).toContain(preset.stream);
      expect(preset.stream).toMatch(/^sample_app_(logs|traces)$/);
    }
    expect(PRESETS.logs[0]?.id).toBe('log-errors');
    expect(PRESETS.traces[0]?.id).toBe('trace-errors');
  });
});
