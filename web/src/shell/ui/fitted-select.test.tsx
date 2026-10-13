import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import { FITTED_SELECT_SEPARATOR, FittedSelect, sizingLabels } from './fitted-select';

afterEach(cleanup);

describe('sizingLabels', () => {
  it('puts the longest labels first, a CJK character counting for two', () => {
    expect(sizingLabels(['app', 'application_logs', '网关访问日志（生产环境）', 'nginx'])).toEqual([
      '网关访问日志（生产环境）',
      'application_logs',
      'nginx',
      'app',
    ]);
  });

  it('names each once and stops at the fifty longest', () => {
    const many = Array.from({ length: 300 }, (_, index) => `label_${'x'.repeat(index)}`);

    expect(sizingLabels(['app', 'app', 'nginx'])).toEqual(['nginx', 'app']);
    expect(sizingLabels(many)).toHaveLength(50);
    expect(sizingLabels(many)[0]).toBe(many[299]);
  });
});

describe('FittedSelect', () => {
  it('is sized by its choices without saying them, up to the width it is given', () => {
    render(
      <FittedSelect
        aria-label="Service"
        value="checkout"
        onValueChange={() => {}}
        maxWidth={240}
        placeholder="Pick a service"
        options={[
          { value: 'checkout', label: 'checkout' },
          FITTED_SELECT_SEPARATOR,
          { value: 'payments', label: 'payments-gateway-internal', muted: true },
        ]}
      />,
    );

    const field = screen.getByRole('combobox', { name: 'Service' });
    const sizers = [...field.querySelectorAll('[data-fitted-sizer]')];
    // The placeholder is one of the things the field may have to show.
    expect(sizers.map((sizer) => sizer.getAttribute('data-label'))).toEqual([
      'payments-gateway-internal',
      'Pick a service',
      'checkout',
    ]);
    // Drawn by CSS from an attribute: no text of the field, nothing for a reader.
    expect(sizers.every((sizer) => sizer.closest('[aria-hidden="true"]') !== null)).toBe(true);
    expect(sizers.every((sizer) => sizer.textContent === '')).toBe(true);
    // Sized in the weight the chosen row of the list is drawn in, the widest any row gets.
    expect(sizers.every((sizer) => sizer.className.split(' ').includes('font-strong'))).toBe(true);
    expect(field.className.split(' ')).toContain('w-auto');
    expect(field.style.maxWidth).toBe('240px');
    expect(field.textContent).toBe('checkout');
  });

  it('says what its value is a value of, and makes room for saying it', () => {
    render(
      <FittedSelect
        aria-label="Version"
        value="v2.4.0"
        onValueChange={() => {}}
        maxWidth={220}
        prefix="Version"
        options={[
          { value: 'all', label: 'All versions' },
          { value: 'v2.4.0', label: 'v2.4.0' },
        ]}
      />,
    );

    const field = screen.getByRole('combobox', { name: 'Version' });
    expect(field.querySelector('[data-fitted-prefix]')!.textContent).toBe('Version');
    expect(field.textContent).toBe('Versionv2.4.0');
    // Each choice is measured with the word before it, as it would stand in the field.
    const measured = [...field.querySelectorAll('[data-fitted-sizer]')].map((sizer) =>
      [...sizer.parentElement!.querySelectorAll('[data-label]')].map((part) => part.getAttribute('data-label')),
    );
    expect(measured).toEqual([
      ['Version', 'All versions'],
      ['Version', 'v2.4.0'],
    ]);
  });
});
