import { describe, expect, it } from 'vitest';

import { colorKeyForService, colorKeyForServiceKind } from './serviceColors';

describe('service colors', () => {
  it('keeps service hashes stable and service kinds centralized', () => {
    expect(colorKeyForService('checkout')).toBe(colorKeyForService('checkout'));
    expect(colorKeyForServiceKind('python')).toBe('--green');
    expect(colorKeyForServiceKind('unknown-runtime')).toBe('--accent');
  });
});
