import { describe, expect, it } from 'vitest';

import { formatFunctionSource, formatSampleInput, parseSampleInput } from './workbench';

describe('function workbench helpers', () => {
  it('parses an empty sample as an empty event', () => {
    expect(parseSampleInput('  ')).toEqual({});
  });

  it('formats valid sample JSON with two-space indentation', () => {
    expect(formatSampleInput('{"level":"info","nested":{"ok":true}}')).toBe(
      '{\n  "level": "info",\n  "nested": {\n    "ok": true\n  }\n}',
    );
  });

  it('normalizes source without rewriting executable content', () => {
    expect(formatFunctionSource('\r\n.value = 1  \r\n.message = "ok"\t\r\n\r\n')).toBe(
      '.value = 1\n.message = "ok"',
    );
  });
});
