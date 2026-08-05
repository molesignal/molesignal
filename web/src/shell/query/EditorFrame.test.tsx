import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import * as React from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { QueryEditorFrame } from '@/shell/query/EditorFrame';

vi.mock('@/shell/codeEditor', () => ({
  CodeEditor: ({ value }: { value: string }) => (
    <div data-testid="query-editor-value">{value}</div>
  ),
}));

afterEach(() => {
  cleanup();
});

function QueryEditorHarness({ initialValue }: { initialValue: string }) {
  const [value, setValue] = React.useState(initialValue);

  return (
    <QueryEditorFrame
      value={value}
      onChange={setValue}
      onClear={() => setValue('')}
      clearLabel="Clear query"
      onModEnter={() => undefined}
      onCollapsedChange={() => undefined}
      collapseLabel="Collapse query"
    />
  );
}

describe('QueryEditorFrame clear action', () => {
  it('clears only the current editor value', () => {
    render(<QueryEditorHarness initialValue="service = 'checkout'" />);

    const clearButton = screen.getByRole('button', { name: 'Clear query' });
    expect((clearButton as HTMLButtonElement).disabled).toBe(false);

    fireEvent.click(clearButton);

    expect(screen.getByTestId('query-editor-value').textContent).toBe('');
    expect(
      (screen.getByRole('button', { name: 'Clear query' }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
  });

  it('shows the clear action as disabled when the editor is empty', () => {
    render(<QueryEditorHarness initialValue="   " />);

    const clearButton = screen.getByRole('button', { name: 'Clear query' });
    expect((clearButton as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(clearButton);
    expect(screen.getByTestId('query-editor-value').textContent).toBe('   ');
  });
});
