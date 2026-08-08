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
    expect(editor.getAttribute('data-font-weight')).toBe('600');
    expect(editor.getAttribute('data-line-height')).toBe('20');
  });

  it('supports page-specific code editor typography', () => {
    render(
      <QueryEditorHarness
        initialValue="rate(http_requests_total[5m])"
        fontSize={14}
        fontWeight={500}
      />,
    );

    const editor = screen.getByTestId('query-editor-value');
    expect(editor.getAttribute('data-font-size')).toBe('14');
    expect(editor.getAttribute('data-font-weight')).toBe('500');
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
