import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { EmptyIllustration } from '@/shell/EmptyIllustration';
import { EmptyState } from '@/shell/EmptyState';

afterEach(cleanup);

function renderState(props: Partial<React.ComponentProps<typeof EmptyState>> = {}) {
  return render(
    <MemoryRouter>
      <EmptyState title="Nothing here" {...props} />
    </MemoryRouter>,
  );
}

describe('EmptyState', () => {
  it('keeps owning its whole region by default', () => {
    renderState({ description: 'Why' });

    const state = screen.getByRole('status');
    expect(state.getAttribute('data-size')).toBe('default');
    expect(state.className).toContain('min-h-[280px]');
    expect(screen.getByRole('heading', { level: 2, name: 'Nothing here' })).not.toBeNull();
  });

  it('fits inside a card when compact: no forced height, a quieter heading', () => {
    renderState({ size: 'compact' });

    const state = screen.getByRole('status');
    expect(state.getAttribute('data-size')).toBe('compact');
    expect(state.className).toContain('min-h-0');
    expect(state.className).not.toContain('min-h-[280px]');
    expect(screen.getByRole('heading', { level: 3, name: 'Nothing here' })).not.toBeNull();
  });

  it('keeps the call to action neutral inside a card so the page keeps one primary action', () => {
    const onClick = vi.fn();
    renderState({ size: 'compact', primaryAction: { label: 'Set up', onClick } });

    const button = screen.getByRole('button', { name: 'Set up' });
    // (the neutral style mentions indigo only for keyboard focus)
    expect(button.classList.contains('bg-indigo')).toBe(false);
    fireEvent.click(button);
    expect(onClick).toHaveBeenCalledOnce();
  });

  it('uses the primary style for the call to action at page level', () => {
    renderState({ primaryAction: { label: 'Set up', onClick: vi.fn() } });
    expect(
      screen.getByRole('button', { name: 'Set up' }).classList.contains('bg-indigo'),
    ).toBe(true);
  });

  it('draws no border on its buttons: a fill is enough', () => {
    renderState({
      primaryAction: { label: 'Primary', onClick: vi.fn() },
      secondaryAction: { label: 'Secondary', onClick: vi.fn() },
    });
    for (const name of ['Primary', 'Secondary']) {
      const classes = screen.getByRole('button', { name }).classList;
      expect(classes.contains('border'), name).toBe(false);
    }
  });

  it('keeps a disabled call to action borderless too', () => {
    renderState({
      size: 'compact',
      primaryAction: { label: 'Set up', onClick: vi.fn(), disabled: true },
    });
    const button = screen.getByRole('button', { name: 'Set up' });
    expect(button.className).not.toMatch(/(^|\s)border(-|\s|$)/);
  });

  it('swaps the icon for an illustration and hides it from assistive tech', () => {
    const { container } = renderState({
      size: 'compact',
      illustration: <EmptyIllustration kind="schedule" />,
    });

    const svgs = container.querySelectorAll('svg');
    expect(svgs).toHaveLength(1);
    expect(svgs[0]?.getAttribute('aria-hidden')).toBe('true');
  });
});
