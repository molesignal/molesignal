import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';
import {
  DateTimePicker,
  formatLocalDateTime,
  parseLocalDateTime,
} from '@/shell/DateTimePicker';

describe.sequential('DateTimePicker', () => {
  afterEach(async () => {
    cleanup();
    vi.useRealTimers();
    await i18n.changeLanguage('en-us');
  });

  it('round-trips strict local wall-clock values', () => {
    const parsed = parseLocalDateTime('2026-07-28T15:11:09');

    expect(parsed).not.toBeNull();
    expect(formatLocalDateTime(parsed!, true)).toBe(
      '2026-07-28T15:11:09',
    );
    expect(parseLocalDateTime('2026-02-30T15:11')).toBeNull();
  });

  it('renders the panel in the application language, not the browser language', async () => {
    await i18n.changeLanguage('zh-cn');
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 6, 28, 12));

    render(
      <DateTimePicker
        value="2026-07-28T15:11"
        onChange={vi.fn()}
      />,
    );

    const trigger = screen.getByRole('button', {
      name: /打开日期时间选择器/,
    });
    expect(trigger.textContent).toContain('2026年7月28日');

    fireEvent.click(trigger);

    expect(
      document.querySelector('[data-slot="date-time-picker-content"]')
        ?.className,
    ).toContain('w-[var(--radix-popover-trigger-width)]');
    expect(screen.getByRole('grid').className).toContain('w-full');
    const selectedDay = document.querySelector(
      '[data-selected-single="true"]',
    );
    expect(selectedDay?.className).toContain('size-[--cell-size]');
    expect(selectedDay?.className).toContain('rounded-full');
    expect(selectedDay?.className).not.toContain('ring-');
    expect(
      document.querySelector('.rdp-today')?.className,
    ).toContain('text-tx-0');
    expect(screen.getByText('2026年7月')).not.toBeNull();
    expect(screen.getByRole('button', { name: '下个月' })).not.toBeNull();
    expect(screen.getByText('时')).not.toBeNull();
    expect(screen.getByText('分')).not.toBeNull();
  });

  it('commits a framework-calendar selection without a native date input', async () => {
    await i18n.changeLanguage('en-us');
    const user = userEvent.setup();
    const onChange = vi.fn();
    const { container } = render(
      <DateTimePicker
        value="2026-07-28T15:11"
        onChange={onChange}
      />,
    );

    await user.click(
      screen.getByRole('button', {
        name: /Open date and time picker/,
      }),
    );
    await user.click(
      screen.getByRole('button', {
        name: /Wednesday, July 29, 2026/,
      }),
    );
    await user.click(screen.getByRole('button', { name: 'Apply' }));

    expect(onChange).toHaveBeenCalledWith('2026-07-29T15:11');
    expect(
      container.querySelector(
        'input[type="date"], input[type="datetime-local"], input[type="time"]',
      ),
    ).toBeNull();
  });
});
