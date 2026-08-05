import { describe, expect, it } from 'vitest';

import { reorderVisibleLogFields } from './ColumnMenu';

describe('log column ordering', () => {
  it('moves a visible field before or after its drop target', () => {
    const fields = ['level', 'service.name', 'message', 'trace_id'];

    expect(reorderVisibleLogFields(fields, 'message', 'level', 'before')).toEqual([
      'message',
      'level',
      'service.name',
      'trace_id',
    ]);
    expect(reorderVisibleLogFields(fields, 'level', 'message', 'after')).toEqual([
      'service.name',
      'message',
      'level',
      'trace_id',
    ]);
  });

  it('leaves the order unchanged for invalid or identical fields', () => {
    const fields = ['level', 'message'];

    expect(reorderVisibleLogFields(fields, 'level', 'level', 'before')).toBe(fields);
    expect(reorderVisibleLogFields(fields, 'missing', 'message', 'before')).toBe(fields);
  });
});
