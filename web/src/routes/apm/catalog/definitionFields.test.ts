import { describe, expect, it } from 'vitest';

import { apdexMillisText, formatTags, isRepositoryAddress, parseApdexMillis, parseTags } from './definitionFields';

describe('the fields of a service definition', () => {
  it('reads tags typed as key=value or a key alone, a comma between each', () => {
    expect(parseTags(' domain=orders , pci,owner=Payments EU ,')).toEqual({
      tags: { domain: 'orders', pci: '', owner: 'Payments EU' },
      problem: null,
    });
    expect(parseTags('')).toEqual({ tags: {}, problem: null });
    expect(formatTags({ pci: '', domain: 'orders' })).toBe('domain=orders, pci');
  });

  it('says what is wrong with a tag the backend would refuse', () => {
    expect(parseTags('Domain=orders')).toMatchObject({ problem: 'key', at: 'Domain' });
    expect(parseTags('team area=x')).toMatchObject({ problem: 'key' });
    expect(parseTags('a=1, a=2')).toMatchObject({ problem: 'twice', at: 'a' });
    expect(parseTags(`a=${'x'.repeat(129)}`)).toMatchObject({ problem: 'value', at: 'a' });
    expect(parseTags(Array.from({ length: 33 }, (_, index) => `k${index}`).join(','))).toMatchObject({ problem: 'too_many' });
  });

  it('reads an Apdex threshold in milliseconds, empty for the default', () => {
    expect(parseApdexMillis('300')).toEqual({ micros: 300_000, valid: true });
    expect(parseApdexMillis(' ')).toEqual({ micros: null, valid: true });
    expect(parseApdexMillis('0').valid).toBe(false);
    expect(parseApdexMillis('61000').valid).toBe(false);
    expect(parseApdexMillis('abc').valid).toBe(false);
    expect(apdexMillisText(250_000)).toBe('250');
    expect(apdexMillisText(null)).toBe('');
  });

  it('takes a repository as an https or a git address', () => {
    for (const address of [
      '',
      'https://github.com/acme/checkout',
      'https://deploy:s3cr3t@git.corp.example/team/checkout.git',
      'git@github.com:acme/checkout.git',
      'ssh://git@git.corp.example:2222/team/checkout.git',
      'git://git.kernel.org/pub/scm/git/git.git',
    ]) {
      expect(isRepositoryAddress(address), address).toBe(true);
    }

    for (const text of ['github.com/acme/checkout', 'ftp://example.com/repo.git', 'git@github.com:', 'https://exa mple.com/repo']) {
      expect(isRepositoryAddress(text), text).toBe(false);
    }
  });
});
