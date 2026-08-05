import { describe, expect, it } from 'vitest';

import { versionComparisonTone } from './Deployments';

describe('version comparison semantics', () => {
  it('keeps insufficient data neutral', () => {
    expect(versionComparisonTone('insufficient_data')).toBe('neutral');
    expect(versionComparisonTone('neutral')).toBe('neutral');
  });

  it('distinguishes regressions from improvements', () => {
    expect(versionComparisonTone('regressed')).toBe('danger');
    expect(versionComparisonTone('improved')).toBe('good');
  });
});
