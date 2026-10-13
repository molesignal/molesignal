import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';

import { TimezoneField, timezoneFieldOptions } from './TimezoneField';

describe('TimezoneField', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-us');
    // The list scrolls its active item into view; jsdom has nothing to scroll.
    Element.prototype.scrollIntoView = vi.fn();
  });

  afterEach(() => cleanup());

  it('lists every zone with UTC first, and never the "follow the browser" entry of a preference', () => {
    const options = timezoneFieldOptions('UTC');

    expect(options[0]).toEqual({ value: 'UTC', label: 'UTC' });
    expect(options.some((option) => option.value === 'Asia/Shanghai')).toBe(true);
    expect(options.find((option) => option.value === 'Asia/Shanghai')?.label).toBe('Asia/Shanghai · UTC+8');
    expect(options.every((option) => option.value !== '__follow__' && option.value !== '')).toBe(true);
    // Each zone once.
    expect(new Set(options.map((option) => option.value)).size).toBe(options.length);
  });

  it('keeps a zone that is set but not in the browser list, so opening a form changes nothing', () => {
    const options = timezoneFieldOptions('Mars/Olympus_Mons');

    expect(options[0]).toEqual({ value: 'Mars/Olympus_Mons', label: 'Mars/Olympus_Mons' });
    expect(options[1]).toEqual({ value: 'UTC', label: 'UTC' });
  });

  it('is a dropdown that is searched and chosen from, not typed into', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<TimezoneField label="Timezone" value="UTC" onChange={onChange} />);

    const field = screen.getByRole('combobox', { name: 'Timezone' });
    expect(field.tagName).toBe('BUTTON');
    expect(field.textContent).toBe('UTC');
    expect(document.querySelector('input')).toBeNull();

    await user.click(field);
    await user.type(screen.getByPlaceholderText('Search timezones…'), 'shanghai');
    const list = screen.getByRole('listbox');
    await user.click(within(list).getByText('Asia/Shanghai · UTC+8'));

    expect(onChange).toHaveBeenCalledWith('Asia/Shanghai');
    expect(screen.queryByRole('listbox')).toBeNull();
  });

  it('shows the zone it holds with its offset, and cannot be opened while disabled', async () => {
    const user = userEvent.setup();
    render(<TimezoneField label="Timezone" value="Asia/Tokyo" onChange={() => undefined} disabled />);

    const field = screen.getByRole('combobox', { name: 'Timezone' }) as HTMLButtonElement;
    expect(field.textContent).toBe('Asia/Tokyo · UTC+9');
    expect(field.disabled).toBe(true);

    await user.click(field);
    expect(screen.queryByRole('listbox')).toBeNull();
  });
});
