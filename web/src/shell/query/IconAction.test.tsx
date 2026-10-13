import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { RefreshCw } from 'lucide-react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { TooltipProvider } from '@/shell/ui/tooltip';

import { QueryIconAction, QueryRunAction } from './IconAction';

afterEach(cleanup);

function renderAction(props: Partial<React.ComponentProps<typeof QueryIconAction>> = {}) {
  return render(
    <TooltipProvider delayDuration={0}>
      <QueryIconAction icon={RefreshCw} label="Refresh" {...props} />
    </TooltipProvider>,
  );
}

describe('QueryIconAction', () => {
  it('is a sign without a word: the word is its name', () => {
    renderAction();

    const button = screen.getByRole('button', { name: 'Refresh' });
    expect(button.textContent).toBe('');
    expect(button.querySelector('svg')?.getAttribute('aria-hidden')).toBe('true');
    // A gray button like the labelled ones beside it, not a bare icon.
    expect(button.className).toContain('bg-bg-2');
  });

  it('says the word in a tip when the keyboard reaches it', async () => {
    const user = userEvent.setup();
    renderAction();

    await user.tab();

    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Refresh' }));
    expect((await screen.findByRole('tooltip')).textContent).toBe('Refresh');
  });

  it('acts when pressed and not while it cannot', async () => {
    const user = userEvent.setup();
    const onClick = vi.fn();
    const { rerender } = renderAction({ onClick });

    await user.click(screen.getByRole('button', { name: 'Refresh' }));
    expect(onClick).toHaveBeenCalledTimes(1);

    rerender(
      <TooltipProvider delayDuration={0}>
        <QueryIconAction icon={RefreshCw} label="Refresh" onClick={onClick} disabled />
      </TooltipProvider>,
    );

    expect((screen.getByRole('button', { name: 'Refresh' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('takes what a trigger gives it, so it can open a panel', () => {
    renderAction({ 'aria-expanded': false, 'aria-haspopup': 'dialog' });

    const button = screen.getByRole('button', { name: 'Refresh' });
    expect(button.getAttribute('aria-haspopup')).toBe('dialog');
    expect(button.getAttribute('aria-expanded')).toBe('false');
  });

  it('keeps its tip to itself when what it opened hands focus back', async () => {
    const { rerender } = renderAction({ 'aria-expanded': true });
    const button = screen.getByRole('button', { name: 'Refresh' });

    // The menu closes, and gives focus back to the button that opened it.
    rerender(
      <TooltipProvider delayDuration={0}>
        <QueryIconAction icon={RefreshCw} label="Refresh" aria-expanded={false} />
      </TooltipProvider>,
    );
    button.focus();
    await Promise.resolve();

    expect(document.activeElement).toBe(button);
    expect(screen.queryByRole('tooltip')).toBeNull();
  });

  it('marks its corner when what it acts on has changed', () => {
    const { rerender } = renderAction();
    expect(document.querySelector('[data-query-action-mark]')).toBeNull();

    rerender(
      <TooltipProvider delayDuration={0}>
        <QueryIconAction icon={RefreshCw} label="Refresh" marked />
      </TooltipProvider>,
    );

    expect(document.querySelector('[data-query-action-mark]')?.getAttribute('aria-hidden')).toBe('true');
    // The mark is a sign: the button still has no text.
    expect(screen.getByRole('button', { name: 'Refresh' }).textContent).toBe('');
  });
});

describe('QueryRunAction', () => {
  function run(props: Partial<React.ComponentProps<typeof QueryRunAction>> = {}) {
    return (
      <TooltipProvider delayDuration={0}>
        <QueryRunAction label="Run" runningLabel="Running…" running={false} {...props} />
      </TooltipProvider>
    );
  }

  it('is the filled one of the row, a sign named by the page', () => {
    render(run());

    const button = screen.getByRole('button', { name: 'Run' });
    expect(button.textContent).toBe('');
    expect(button.className).toContain('bg-indigo');
    // The same edge as the gray ones, so it is the box they are.
    expect(button.className.split(' ')).toEqual(expect.arrayContaining(['border', 'border-transparent']));
  });

  it('turns its sign and says so while the query runs', () => {
    const { rerender } = render(run());
    expect(screen.getByRole('button', { name: 'Run' }).querySelector('svg')?.getAttribute('class')).not.toContain(
      'animate-spin',
    );

    rerender(run({ running: true }));

    const button = screen.getByRole('button', { name: 'Running…' });
    expect(button.querySelector('svg')?.getAttribute('class')).toContain('animate-spin');
  });

  it('marks a query that was changed since it ran', () => {
    render(run({ changed: true }));

    expect(screen.getByRole('button', { name: 'Run' }).querySelector('[data-query-action-mark]')).not.toBeNull();
  });
});
