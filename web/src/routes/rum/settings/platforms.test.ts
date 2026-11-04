import { describe, expect, it } from 'vitest';

import { platformName } from './platforms';

describe('platformName', () => {
  it('writes the first letter in capitals, and iOS as its makers write it', () => {
    expect(platformName('web')).toBe('Web');
    expect(platformName('flutter')).toBe('Flutter');
    expect(platformName('android')).toBe('Android');
    expect(platformName('ios')).toBe('iOS');
  });

  it('capitalizes a platform it has no name for', () => {
    expect(platformName('harmony')).toBe('Harmony');
    expect(platformName('')).toBe('');
  });
});
