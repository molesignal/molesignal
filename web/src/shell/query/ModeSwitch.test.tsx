import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';

import { QueryModeSwitch } from './ModeSwitch';

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
});

afterEach(() => {
  cleanup();
});

describe('QueryModeSwitch', () => {
  it('offers Code and Builder, in that order, and marks the one in use', () => {
    render(<QueryModeSwitch mode="code" onChange={vi.fn()} />);

    const group = screen.getByRole('group', { name: 'Query editor mode' });
    const buttons = Array.from(group.querySelectorAll('button'));
    expect(buttons.map((button) => button.textContent)).toEqual(['Code', 'Builder']);
    expect(buttons.map((button) => button.getAttribute('aria-pressed'))).toEqual(['true', 'false']);
  });

  it('asks for the other mode when it is picked', () => {
    const onChange = vi.fn();
    render(<QueryModeSwitch mode="code" onChange={onChange} />);

    fireEvent.click(screen.getByRole('button', { name: 'Builder' }));

    expect(onChange).toHaveBeenCalledWith('builder');
  });

  it('reads the same in every explorer language', async () => {
    await i18n.changeLanguage('zh-cn');
    render(<QueryModeSwitch mode="builder" onChange={vi.fn()} />);

    expect(screen.getByRole('button', { name: '代码' }).getAttribute('aria-pressed')).toBe('false');
    expect(screen.getByRole('button', { name: '构造器' }).getAttribute('aria-pressed')).toBe('true');
  });
});
