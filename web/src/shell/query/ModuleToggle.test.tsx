import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { BarChart3 } from 'lucide-react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { TooltipProvider } from '@/shell/ui/tooltip';

import { ModuleToggle } from './ModuleToggle';

afterEach(cleanup);

const LABELS = {
  hide: 'Hide histogram',
  show: 'Show histogram',
} as const;

function toggle(open: boolean, onOpenChange: (open: boolean) => void) {
  return (
    <TooltipProvider>
      <ModuleToggle
        open={open}
        icon={BarChart3}
        actionLabel={open ? LABELS.hide : LABELS.show}
        onOpenChange={onOpenChange}
      />
    </TooltipProvider>
  );
}

describe('ModuleToggle', () => {
  it('is a sign without a word, named by what pressing it will do', () => {
    const onOpenChange = vi.fn();
    render(toggle(true, onOpenChange));

    const button = screen.getByRole('button', { name: LABELS.hide });
    expect(button.textContent).toBe('');
    expect(button.getAttribute('aria-pressed')).toBe('true');
    expect(button.getAttribute('data-module-state')).toBe('open');

    fireEvent.click(button);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it('stays a gray button when closed: the sign goes out and dims', () => {
    const onOpenChange = vi.fn();
    const { rerender } = render(toggle(true, onOpenChange));
    expect(screen.getByRole('button').querySelector('svg')?.getAttribute('class')).toContain(
      'text-indigo',
    );

    rerender(toggle(false, onOpenChange));

    const button = screen.getByRole('button', { name: LABELS.show });
    expect(button.getAttribute('aria-pressed')).toBe('false');
    expect(button.className).toContain('text-tx-3');
    // No tint of its own: it keeps the fill of the buttons beside it.
    expect(button.className).toContain('bg-bg-2');
    expect(button.querySelector('svg')?.getAttribute('class')).not.toContain('text-indigo');

    fireEvent.click(button);
    expect(onOpenChange).toHaveBeenCalledWith(true);
  });
});
