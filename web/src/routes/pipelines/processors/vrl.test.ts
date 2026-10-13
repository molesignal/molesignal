import { describe, expect, it } from 'vitest';

import { onField, vrlPath, vrlReadPaths, vrlRegex, vrlString } from './vrl';

describe('VRL paths', () => {
  it('writes a field the way people name it', () => {
    expect(vrlPath('level')).toBe('.level');
    expect(vrlPath(' kubernetes.labels.app ')).toBe('.kubernetes.labels.app');
    expect(vrlPath('_timestamp')).toBe('._timestamp');
  });

  it('quotes what is not an identifier', () => {
    expect(vrlPath('pod-name')).toBe('."pod-name"');
    expect(vrlPath('my field')).toBe('."my field"');
    expect(vrlPath('0day')).toBe('."0day"');
    expect(vrlPath('a"b\\c')).toBe('."a\\"b\\\\c"');
    expect(vrlPath('k8s.pod-name')).toBe('.k8s."pod-name"');
  });

  it('ignores empty segments', () => {
    expect(vrlPath('a..b.')).toBe('.a.b');
    expect(vrlPath('  ')).toBe('.');
  });

  it('reads a dotted name as one key before it reads it as a path', () => {
    expect(vrlReadPaths('level')).toEqual(['.level']);
    expect(vrlReadPaths('service.name')).toEqual(['."service.name"', '.service.name']);
  });
});

describe('VRL literals', () => {
  it('escapes what a quoted string cannot hold as it is', () => {
    expect(vrlString('prod')).toBe('"prod"');
    expect(vrlString('say "hi" \\ there')).toBe('"say \\"hi\\" \\\\ there"');
    expect(vrlString('a\nb\tc\rd')).toBe('"a\\nb\\tc\\rd"');
  });

  it('keeps a quoted string from being read as a template', () => {
    expect(vrlString('{{ env }}')).toBe('"\\{{ env }}"');
  });

  it('writes other characters as they are, since VRL has no unicode escape', () => {
    expect(vrlString('多字节 é')).toBe('"多字节 é"');
    expect(vrlString('a\u0001b')).toBe('"ab"');
  });

  it('escapes only the quote that would end a regex literal', () => {
    expect(vrlRegex('^(?P<n>\\d+)$')).toBe("r'^(?P<n>\\d+)$'");
    expect(vrlRegex("user='(?P<u>[^']+)'")).toBe("r'user=\\'(?P<u>[^\\']+)\\''");
  });
});

describe('onField', () => {
  it('guards the one place a plain field can be', () => {
    expect(onField('level', 'is_string', (path) => [`${path} = downcase(string!(${path}))`])).toEqual([
      'if is_string(.level) {',
      '  .level = downcase(string!(.level))',
      '}',
    ]);
  });

  it('tries the key, then the path, for a dotted name', () => {
    expect(onField('log.level', 'exists', (path) => [`del(${path})`])).toEqual([
      'if exists(."log.level") {',
      '  del(."log.level")',
      '} else if exists(.log.level) {',
      '  del(.log.level)',
      '}',
    ]);
  });
});
