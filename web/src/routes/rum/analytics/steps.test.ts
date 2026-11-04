import { describe, expect, it } from 'vitest';

import { decodeSteps, defaultField, emptyStep, encodeSteps, isComplete, MAX_STEPS, type StepDraft, toStep } from './steps';

const page = (value: string): StepDraft => ({ kind: 'views', field: 'view.url_path_group', op: '=', value });

describe('the steps of a funnel in an address', () => {
  it('are written as kind, field, comparison and value, and read back', () => {
    const steps: StepDraft[] = [
      page('/'),
      page('/products?a=1|2'),
      { kind: 'actions', field: 'action.target.name', op: 'contains', value: 'Pay now' },
    ];

    const text = encodeSteps(steps);

    expect(text).toBe('v:view.url_path_group=%2F|v:view.url_path_group=%2Fproducts%3Fa%3D1%7C2|a:action.target.name~Pay%20now');
    expect(decodeSteps(text)).toEqual(steps);
  });

  it('are none where the address says none', () => {
    expect(decodeSteps(null)).toEqual([]);
    expect(decodeSteps('')).toEqual([]);
  });

  it('leave out what is not a step, a field the kind has none of, and text that cannot be read', () => {
    expect(decodeSteps('nonsense|v:view.url_path_group=%2F|a:view.url_path~x|v:view.url_path_group=%E0%A4%A')).toEqual([page('/')]);
  });

  it('are at most as many as the server takes', () => {
    const many = Array.from({ length: MAX_STEPS + 3 }, () => page('/')).map((step) => encodeSteps([step])).join('|');

    expect(decodeSteps(many)).toHaveLength(MAX_STEPS);
  });
});

describe('a step', () => {
  it('begins with the field of its kind and the platform', () => {
    expect(defaultField('views', true)).toBe('view.url_path_group');
    expect(defaultField('views', false)).toBe('view.name');
    expect(defaultField('actions', true)).toBe('action.target.name');
    expect(emptyStep('views', true)).toEqual({ kind: 'views', field: 'view.url_path_group', op: '=', value: '' });
  });

  it('is ready when it names what to look for', () => {
    expect(isComplete(page(''))).toBe(false);
    expect(isComplete(page('  '))).toBe(false);
    expect(isComplete(page('/'))).toBe(true);
  });

  it('goes to the server as a kind and one filter, the value trimmed', () => {
    expect(toStep({ kind: 'actions', field: 'action.target.name', op: 'contains', value: ' Pay ' })).toEqual({
      type: 'actions',
      filters: [{ field: 'action.target.name', op: 'contains', value: 'Pay' }],
    });
  });
});
