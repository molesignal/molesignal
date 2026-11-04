import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { CodeEditorProps } from '@/shell/codeEditor';

import { JsonEditor, JsonView } from './JsonEditor';

// Monaco is the editor in a browser; here it is a field that says what it was given.
vi.mock('@/shell/codeEditor', () => ({
  CodeEditor: ({
    value,
    onChange,
    ariaLabel,
    language,
    readOnly,
    showHeader,
    showStatus,
    lineNumbers,
    minHeight,
  }: CodeEditorProps) => (
    <textarea
      aria-label={ariaLabel}
      value={value}
      readOnly={readOnly}
      data-language={language}
      data-header={String(showHeader)}
      data-status={String(showStatus)}
      data-line-numbers={String(lineNumbers)}
      data-min-height={minHeight}
      onChange={(event) => onChange?.(event.target.value)}
    />
  ),
}));

afterEach(cleanup);

describe('JsonEditor', () => {
  it('is the code editor in JSON, a field without the bars above and below it or line numbers', () => {
    const onChange = vi.fn();
    render(<JsonEditor value="{}" onChange={onChange} ariaLabel="Matchers" />);

    const editor = screen.getByRole<HTMLTextAreaElement>('textbox', { name: 'Matchers' });
    expect(editor.dataset.language).toBe('json');
    expect(editor.dataset.header).toBe('false');
    expect(editor.dataset.status).toBe('false');
    expect(editor.dataset.lineNumbers).toBe('false');
    expect(editor.readOnly).toBe(false);

    fireEvent.change(editor, { target: { value: '{"severity": "critical"}' } });
    expect(onChange).toHaveBeenCalledWith('{"severity": "critical"}');
  });

  it('keeps the bars a page asks for', () => {
    render(<JsonEditor value="{}" ariaLabel="Definition" showHeader showStatus />);

    const editor = screen.getByRole<HTMLTextAreaElement>('textbox', { name: 'Definition' });
    expect(editor.dataset.header).toBe('true');
    expect(editor.dataset.status).toBe('true');
  });

  it('shows no line numbers, read only too', () => {
    render(<JsonView value={{ ok: true }} ariaLabel="Payload" />);

    expect(screen.getByRole<HTMLTextAreaElement>('textbox', { name: 'Payload' }).dataset.lineNumbers).toBe('false');
  });
});

describe('JsonView', () => {
  it('shows a value as JSON with two-space indents, to read and not to change', () => {
    render(<JsonView value={{ service: 'api', labels: { env: 'production' }, tags: [] }} ariaLabel="Payload" />);

    const view = screen.getByRole<HTMLTextAreaElement>('textbox', { name: 'Payload' });
    expect(view.value).toBe('{\n  "service": "api",\n  "labels": {\n    "env": "production"\n  },\n  "tags": []\n}');
    expect(view.readOnly).toBe(true);
    expect(view.dataset.language).toBe('json');
  });

  it('is a line tall at the least, and empty for a value JSON has no text for', () => {
    render(<JsonView value={undefined} ariaLabel="Nothing" />);

    const view = screen.getByRole<HTMLTextAreaElement>('textbox', { name: 'Nothing' });
    expect(view.value).toBe('');
    expect(view.dataset.minHeight).toBe('36');
  });
});
