import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import * as React from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { QueryEditorFrame } from '@/shell/query/EditorFrame';

vi.mock('@/shell/codeEditor', () => ({
  CodeEditor: ({
    value,
    fontSize,
    fontWeight,
    lineHeight,
  }: {
    value: string;
    fontSize?: number;
    fontWeight?: number;
    lineHeight?: number;
  }) => (
    <div
      data-testid="query-editor-value"
      data-font-size={fontSize}
      data-font-weight={fontWeight}
      data-line-height={lineHeight}
    >
      {value}
    </div>
  ),
}));

afterEach(() => {
  cleanup();
});

function QueryEditorHarness({
  initialValue,
  fontSize,
  fontWeight,
}: {
  initialValue: string;
  fontSize?: number;
  fontWeight?: number;
}) {
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
      {...(fontSize !== undefined ? { fontSize } : {})}
      {...(fontWeight !== undefined ? { fontWeight } : {})}
    />
  );
}

describe('QueryEditorFrame clear action', () => {
  it('uses the shared code editor typography defaults', () => {
    render(<QueryEditorHarness initialValue="service = 'checkout'" />);

    const editor = screen.getByTestId('query-editor-value');
    expect(editor.getAttribute('data-font-size')).toBe('12');
    expect(editor.getAttribute('data-font-weight')).toBe('500');
    expect(editor.getAttribute('data-line-height')).toBe('20');
  });

  it('supports page-specific code editor typography', () => {
    render(
      <QueryEditorHarness
        initialValue="rate(http_requests_total[5m])"
        fontSize={14}
        fontWeight={400}
      />,
    );

    const editor = screen.getByTestId('query-editor-value');
    expect(editor.getAttribute('data-font-size')).toBe('14');
    expect(editor.getAttribute('data-font-weight')).toBe('400');
  });

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

describe('QueryEditorFrame query tabs', () => {
  const TABS = (
    <div role="tablist" aria-label="Queries">
      <button type="button" role="tab" aria-selected>
        Query 1
      </button>
    </div>
  );

  it('puts the tabs where the letter and the name of the query stand', () => {
    render(
      <QueryEditorFrame
        value="status = 500"
        onChange={() => undefined}
        onCollapsedChange={() => undefined}
        label="Query"
        tabs={TABS}
      />,
    );

    expect(screen.getByRole('tab', { name: 'Query 1' })).not.toBeNull();
    expect(screen.queryByText('A')).toBeNull();
    expect(screen.queryByText('Query')).toBeNull();
  });

  it('keeps the tabs of a folded frame out of the button that opens it', () => {
    const opened = vi.fn();
    render(
      <QueryEditorFrame
        value="status = 500"
        onChange={() => undefined}
        collapsed
        onCollapsedChange={opened}
        label="Query"
        expandLabel="Expand query"
        summary="status = 500"
        tabs={TABS}
      />,
    );

    const expand = screen.getByRole('button', { name: 'Expand query' });
    const tab = screen.getByRole('tab', { name: 'Query 1' });
    expect(expand.contains(tab)).toBe(false);
    expect(expand.textContent).toContain('status = 500');

    fireEvent.click(tab);
    expect(opened).not.toHaveBeenCalled();
    fireEvent.click(expand);
    expect(opened).toHaveBeenCalledWith(false);
  });
});
