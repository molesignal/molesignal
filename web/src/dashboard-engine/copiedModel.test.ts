import { describe, expect, it } from 'vitest';

import { copiedModel } from './copiedModel';

describe('the copy of a dashboard', () => {
  const builtin = {
    uid: 'rum-browser',
    title: 'RUM',
    editable: false,
    tags: ['内置', 'rum'],
    extensions: { builtin: { key: 'rum-browser', version: 1 }, other: true },
  };

  it('is a new dashboard that can be changed, with the title it is given', () => {
    const copy = copiedModel(builtin, 'RUM (copy)');

    expect(copy.uid).toBe('');
    expect(copy.title).toBe('RUM (copy)');
    expect(copy.editable).toBe(true);
    expect(copy.tags).toEqual(['内置', 'rum']);
  });

  it('is not a built-in dashboard, and keeps the rest of what the model had to say', () => {
    expect(copiedModel(builtin, 'x').extensions).toEqual({ other: true });
    expect(copiedModel({ uid: 'a' }, 'x').extensions).toEqual({});
  });

  it('leaves the model it was made from as it was', () => {
    copiedModel(builtin, 'x');

    expect(builtin.editable).toBe(false);
    expect(builtin.extensions.builtin.version).toBe(1);
  });
});
