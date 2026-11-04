import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import * as React from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import '@/i18n';

import { FieldArray, FormDrawer, FormInput } from '@/shell/FormDrawer';

afterEach(cleanup);

function Drawer({
  onOpenChange,
  footer = <button type="button">Save</button>,
  dirty,
}: {
  onOpenChange: (open: boolean) => void;
  footer?: React.ReactNode;
  dirty?: boolean;
}) {
  const [rows, setRows] = React.useState<string[]>([]);
  return (
    <FormDrawer open onOpenChange={onOpenChange} title="New rule" footer={footer} dirty={dirty}>
      <FormInput aria-label="Name" />
      <button type="button" data-tab="" aria-pressed="false">
        A view
      </button>
      <FieldArray
        items={rows}
        onChange={setRows}
        newItem={() => ''}
        addLabel="Add row"
        renderItem={(row) => <span>{row}</span>}
      />
    </FormDrawer>
  );
}

const closeWithEscape = () => fireEvent.keyDown(screen.getAllByRole('dialog').at(-1)!, { key: 'Escape' });

describe('a form drawer holding a draft', () => {
  it('closes at once while nothing changed', () => {
    const onOpenChange = vi.fn();
    render(<Drawer onOpenChange={onOpenChange} />);

    closeWithEscape();

    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(screen.queryByTestId('unsaved-prompt')).toBeNull();
  });

  it('asks before losing what was typed, and staying keeps it', () => {
    const onOpenChange = vi.fn();
    render(<Drawer onOpenChange={onOpenChange} />);

    fireEvent.input(screen.getByLabelText('Name'), { target: { value: 'half a rule' } });
    closeWithEscape();

    expect(onOpenChange).not.toHaveBeenCalled();
    expect(screen.getByTestId('unsaved-prompt')).not.toBeNull();

    fireEvent.click(screen.getByRole('button', { name: 'Keep editing' }));

    expect(screen.queryByTestId('unsaved-prompt')).toBeNull();
    expect((screen.getByLabelText('Name') as HTMLInputElement).value).toBe('half a rule');
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it('closes once the draft is given up', () => {
    const onOpenChange = vi.fn();
    render(<Drawer onOpenChange={onOpenChange} />);

    fireEvent.input(screen.getByLabelText('Name'), { target: { value: 'half a rule' } });
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    fireEvent.click(screen.getByRole('button', { name: 'Discard' }));

    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it('counts a row added, which no field reports', () => {
    const onOpenChange = vi.fn();
    render(<Drawer onOpenChange={onOpenChange} />);

    fireEvent.click(screen.getByRole('button', { name: 'Add row' }));
    closeWithEscape();

    expect(onOpenChange).not.toHaveBeenCalled();
    expect(screen.getByTestId('unsaved-prompt')).not.toBeNull();
  });

  it('does not count a view tab being pressed', () => {
    const onOpenChange = vi.fn();
    render(<Drawer onOpenChange={onOpenChange} />);

    fireEvent.pointerUp(screen.getByRole('button', { name: 'A view' }));
    closeWithEscape();

    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it('lets the form overrule what was touched', () => {
    const onOpenChange = vi.fn();
    render(<Drawer onOpenChange={onOpenChange} dirty={false} />);

    fireEvent.input(screen.getByLabelText('Name'), { target: { value: 'typed, then reverted' } });
    closeWithEscape();

    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it('never asks in a drawer without a footer: it holds nothing to save', () => {
    const onOpenChange = vi.fn();
    render(
      <FormDrawer open onOpenChange={onOpenChange} title="Details">
        <FormInput aria-label="Search" />
      </FormDrawer>,
    );

    fireEvent.input(screen.getByLabelText('Search'), { target: { value: 'x' } });
    closeWithEscape();

    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});
