import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import type { ReactElement } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';
import { TooltipProvider } from '@/shell/ui/tooltip';

import { QueryModeSwitch } from './ModeSwitch';

/** The switch as a page has it: inside the tooltips' provider. */
const view = (ui: ReactElement) => render(<TooltipProvider>{ui}</TooltipProvider>);

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
});

afterEach(() => {
  cleanup();
});

describe('QueryModeSwitch', () => {
  it('offers Code and Builder as icons, in that order, named, and marks the one in use', () => {
    view(<QueryModeSwitch mode="code" onChange={vi.fn()} />);

    const group = screen.getByRole('group', { name: 'Query editor mode' });
    const buttons = Array.from(group.querySelectorAll('button'));
    // No words in the buttons: each is an icon that names itself.
    expect(buttons.map((button) => button.textContent)).toEqual(['', '']);
    expect(buttons.map((button) => button.getAttribute('aria-label'))).toEqual(['Code', 'Builder']);
    expect(buttons.map((button) => button.querySelector('svg') !== null)).toEqual([true, true]);
    expect(buttons.map((button) => button.getAttribute('aria-pressed'))).toEqual(['true', 'false']);
  });

  it('asks for the other mode when it is picked', () => {
    const onChange = vi.fn();
    view(<QueryModeSwitch mode="code" onChange={onChange} />);

    fireEvent.click(screen.getByRole('button', { name: 'Builder' }));

    expect(onChange).toHaveBeenCalledWith('builder');
  });

  it('reads the same in every explorer language', async () => {
    await i18n.changeLanguage('zh-cn');
    view(<QueryModeSwitch mode="builder" onChange={vi.fn()} />);

    expect(screen.getByRole('button', { name: '代码' }).getAttribute('aria-pressed')).toBe('false');
    expect(screen.getByRole('button', { name: '构造器' }).getAttribute('aria-pressed')).toBe('true');
  });
});
