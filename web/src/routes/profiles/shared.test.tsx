import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import i18n from '@/i18n';

import { compareHref, profileTypesIn, ServiceSelect, TypeSelect } from './shared';

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
});

afterEach(cleanup);

/** What a field is sized by, longest first: the labels it could show. */
const sizedBy = (field: HTMLElement) =>
  [...field.querySelectorAll('[data-fitted-sizer]')].map((sizer) => sizer.getAttribute('data-label'));

describe('profileTypesIn', () => {
  it('lists the types profiles were captured of, in the order types are offered in', () => {
    expect(
      profileTypesIn([
        { profile_type: 'alloc_space' },
        { profile_type: 'cpu' },
        { profile_type: 'alloc_space' },
      ]),
    ).toEqual(['cpu', 'alloc_space']);
  });

  it('puts a type the product does not name after the ones it does', () => {
    expect(profileTypesIn([{ profile_type: 'zz_custom' }, { profile_type: 'lock' }, { profile_type: 'block' }])).toEqual([
      'lock',
      'block',
      'zz_custom',
    ]);
  });

  it('has none for a window that holds nothing', () => {
    expect(profileTypesIn([])).toEqual([]);
  });
});

describe('compareHref', () => {
  it('takes the scope of the analysis along to the comparison', () => {
    expect(compareHref({ service: 'checkout api', type: 'cpu' })).toBe(
      '/profiles/compare?service=checkout+api&type=cpu',
    );
    expect(compareHref({ service: '', type: 'alloc_space' })).toBe('/profiles/compare?type=alloc_space');
    expect(compareHref({})).toBe('/profiles/compare');
  });
});

describe('TypeSelect', () => {
  it('has no "all": a flame graph is of one type', () => {
    render(<TypeSelect value="cpu" onChange={() => {}} />);

    const field = screen.getByRole('combobox', { name: 'Profile type' });
    // The eight types, and what the field says before one is known.
    expect([...sizedBy(field)].sort()).toEqual([
      'Allocated memory',
      'Allocated objects',
      'CPU',
      'Goroutines',
      'In-use memory',
      'In-use objects',
      'Lock contention',
      'Profile type',
      'Wall time',
    ]);
  });

  it('is as wide as its longest choice, up to a width of its own', () => {
    render(<TypeSelect value="cpu" onChange={() => {}} />);

    const field = screen.getByRole('combobox', { name: 'Profile type' });
    expect(sizedBy(field)[0]).toBe('Allocated objects');
    expect(field.className.split(' ')).toContain('w-auto');
    expect(field.style.maxWidth).toBe('240px');
  });

  it('keeps a type nothing was captured of, or that the product does not name, as a choice', () => {
    render(<TypeSelect value="block" onChange={() => {}} present={['cpu']} />);

    expect(sizedBy(screen.getByRole('combobox', { name: 'Profile type' }))).toContain('block');
  });
});

describe('ServiceSelect', () => {
  it('offers every service and all of them together', () => {
    render(<ServiceSelect value="" services={['checkout', 'payments-gateway']} onChange={() => {}} />);

    const field = screen.getByRole('combobox', { name: 'Service' });
    expect(field.textContent).toBe('All services');
    expect(sizedBy(field)).toEqual(['payments-gateway', 'All services', 'checkout']);
  });

  it('keeps the service looked at as a choice though the period has nothing of it', () => {
    render(<ServiceSelect value="inventory" services={['checkout']} onChange={() => {}} />);

    expect(sizedBy(screen.getByRole('combobox', { name: 'Service' }))).toContain('inventory');
  });
});
