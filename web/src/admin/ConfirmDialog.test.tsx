import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import '@/i18n';

import { ConfirmDialog } from './ConfirmDialog';

afterEach(cleanup);

describe('ConfirmDialog', () => {
  it('takes its default words from the translations', () => {
    render(<ConfirmDialog open onOpenChange={vi.fn()} title="Sure?" onConfirm={vi.fn()} />);

    expect(screen.getByRole('button', { name: 'Cancel' })).not.toBeNull();
    expect(screen.getByRole('button', { name: 'Confirm' })).not.toBeNull();
  });

  it('draws a destructive action in the colour made for its red', () => {
    render(<ConfirmDialog open onOpenChange={vi.fn()} title="Sure?" destructive confirmLabel="Delete" onConfirm={vi.fn()} />);

    const className = screen.getByRole('button', { name: 'Delete' }).className;
    expect(className).toContain('bg-red');
    expect(className).toContain('text-[var(--red-fg)]');
    expect(className).not.toContain('text-tx-0');
    expect(className).not.toContain('text-white');
  });

  it('states what goes with the thing', () => {
    render(
      <ConfirmDialog
        open
        onOpenChange={vi.fn()}
        title="Delete the stream?"
        consequences={[{ label: 'Data', value: 'Every event of the stream is erased' }]}
        onConfirm={vi.fn()}
      />,
    );

    expect(screen.getByText('Data')).not.toBeNull();
    expect(screen.getByText('Every event of the stream is erased')).not.toBeNull();
  });

  it('keeps the action closed until the exact name is typed', () => {
    const onConfirm = vi.fn();
    render(
      <ConfirmDialog
        open
        onOpenChange={vi.fn()}
        title="Delete the organization?"
        typeToConfirm="acme-prod"
        confirmLabel="Delete"
        destructive
        onConfirm={onConfirm}
      />,
    );

    const action = () => screen.getByRole('button', { name: 'Delete' }) as HTMLButtonElement;
    const field = screen.getByTestId('confirm-type-name');

    expect(action().disabled).toBe(true);

    fireEvent.change(field, { target: { value: 'ACME-PROD' } });
    expect(action().disabled).toBe(true);

    fireEvent.change(field, { target: { value: 'acme-prod' } });
    expect(action().disabled).toBe(false);

    fireEvent.click(action());
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it('starts over each time it opens', () => {
    const tree = (open: boolean) => (
      <ConfirmDialog open={open} onOpenChange={vi.fn()} title="Delete?" typeToConfirm="a" onConfirm={vi.fn()} />
    );
    const { rerender } = render(tree(true));

    fireEvent.change(screen.getByTestId('confirm-type-name'), { target: { value: 'a' } });
    rerender(tree(false));
    rerender(tree(true));

    expect((screen.getByTestId('confirm-type-name') as HTMLInputElement).value).toBe('');
  });
});
