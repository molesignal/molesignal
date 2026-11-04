import { describe, expect, it } from 'vitest';

import type { DashboardVariable } from './schema';
import {
  addressWithVariables,
  sameVariableValue,
  sameVariableValues,
  variablesInAddress,
} from './variableAddress';

function variable(name: string, extra: Partial<DashboardVariable> = {}): DashboardVariable {
  return {
    id: `variable-${name}`,
    name,
    label: name,
    type: 'custom',
    multi: false,
    includeAll: false,
    hide: 'none',
    refresh: 'never',
    ...extra,
  };
}

const declared = [
  variable('application'),
  variable('environment'),
  variable('regions', { multi: true }),
  variable('fixed', { type: 'constant' }),
];

describe('the variables of a dashboard in its address', () => {
  it('reads the values the address names, and only for variables the dashboard declares', () => {
    const params = new URLSearchParams('time=1h&var-application=shop-web&var-environment=prod&var-other=x');

    expect(variablesInAddress(params, declared)).toEqual({ application: 'shop-web', environment: 'prod' });
  });

  it('reads a name once for each value of a list', () => {
    const params = new URLSearchParams('var-regions=eu&var-regions=us&var-application=a');

    expect(variablesInAddress(params, declared)).toEqual({ application: 'a', regions: ['eu', 'us'] });
  });

  it('takes the first of a name that is repeated for a variable of one value', () => {
    expect(variablesInAddress(new URLSearchParams('var-application=a&var-application=b'), declared)).toEqual({
      application: 'a',
    });
  });

  it('keeps the rest of the address and replaces the variables in it', () => {
    const params = new URLSearchParams('time=1h&var-application=old&var-gone=x');
    const next = addressWithVariables(params, declared, {
      application: 'shop-web',
      environment: '全部',
      regions: ['eu', 'us'],
      fixed: 'nope',
    });

    expect(next.get('time')).toBe('1h');
    expect(next.get('var-gone')).toBe('x');
    expect(next.get('var-application')).toBe('shop-web');
    expect(next.get('var-environment')).toBe('全部');
    expect(next.getAll('var-regions')).toEqual(['eu', 'us']);
    expect(next.has('var-fixed')).toBe(false);
  });

  it('leaves a variable that has no value out of the address', () => {
    const next = addressWithVariables(new URLSearchParams('var-application=a'), declared, { application: '' });

    expect(next.has('var-application')).toBe(false);
  });

  it('compares values as text, whether single or in a list', () => {
    expect(sameVariableValue('15', 15)).toBe(true);
    expect(sameVariableValue(['a'], 'a')).toBe(true);
    expect(sameVariableValue(['a', 'b'], ['a'])).toBe(false);
    expect(sameVariableValue('', undefined)).toBe(true);
    expect(sameVariableValues({ a: '1', b: ['x'] }, { a: 1, b: 'x' })).toBe(true);
    expect(sameVariableValues({ a: '1' }, { a: '1', b: 'x' })).toBe(false);
  });
});
