import { describe, expect, it } from 'vitest';

import { indexOptionsFor } from './model';

const values = (dataType: Parameters<typeof indexOptionsFor>[0]) =>
  indexOptionsFor(dataType).map((option) => option.value);

describe('index options', () => {
  it('offers the numeric range index for numbers and times only', () => {
    for (const dataType of ['int64', 'float64', 'timestamp'] as const) {
      expect(values(dataType)).toContain('range');
    }

    for (const dataType of ['utf8', 'json', 'bool'] as const) {
      expect(values(dataType)).not.toContain('range');
    }
  });
});
