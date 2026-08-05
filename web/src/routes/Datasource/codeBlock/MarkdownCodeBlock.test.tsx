import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { TooltipProvider } from '@/shell/ui/tooltip';

import { MarkdownCodeBlock } from './MarkdownCodeBlock';
import { highlightCode } from './highlighter';

vi.mock('./highlighter', () => ({
  highlightCode: vi.fn(async () => [
    [
      { text: 'const', kind: 'keyword' },
      { text: ' answer ', kind: 'identifier' },
      { text: '=', kind: 'operator' },
      { text: ' 42', kind: 'number' },
      { text: ';', kind: 'punctuation' },
    ],
  ]),
}));

function renderCodeBlock(content = 'const answer = 42;') {
  return render(
    <TooltipProvider delayDuration={0}>
      <MarkdownCodeBlock language="ts" content={content} />
    </TooltipProvider>,
  );
}

describe('MarkdownCodeBlock', () => {
  afterEach(cleanup);

  beforeEach(() => {
    vi.mocked(highlightCode).mockClear();
  });

  it('renders a Markdown-style language block with semantic tokens', async () => {
    const { container } = renderCodeBlock();

    expect(screen.getByText('TypeScript')).not.toBeNull();
    const code = screen.getByLabelText('TypeScript code');
    const pre = container.querySelector('pre');
    expect(code.textContent).toContain('const answer = 42;');
    expect(pre?.className).toContain('overflow-x-auto');
    expect(pre?.className).not.toContain('max-h-');
    expect(pre?.className).not.toContain('overflow-auto');

    await waitFor(() => expect(code.getAttribute('data-highlighted')).toBe('true'));
    expect(code.querySelector('[data-token-kind="keyword"]')?.textContent).toBe('const');
    expect(code.querySelector('[data-token-kind="number"]')?.textContent).toContain('42');
    expect(highlightCode).toHaveBeenCalledWith('const answer = 42;', 'ts');
  });

  it('copies the original unformatted source', async () => {
    const user = userEvent.setup();
    const writeText = vi.spyOn(navigator.clipboard, 'writeText');
    renderCodeBlock('const answer = 42;');

    await user.click(
      screen.getByRole('button', { name: 'datasource_page.copy' }),
    );

    expect(writeText).toHaveBeenCalledWith('const answer = 42;');
  });

  it('collapses and expands the code body from the header', async () => {
    const user = userEvent.setup();
    renderCodeBlock();

    const code = screen.getByLabelText('TypeScript code');
    const pre = code.closest('pre');
    const collapse = screen.getByRole('button', {
      name: 'datasource_page.collapse_code',
    });

    expect(collapse.getAttribute('aria-expanded')).toBe('true');
    expect(pre?.hidden).toBe(false);

    await user.click(collapse);

    const expand = screen.getByRole('button', {
      name: 'datasource_page.expand_code',
    });
    expect(expand.getAttribute('aria-expanded')).toBe('false');
    expect(pre?.hidden).toBe(true);

    await user.click(expand);

    expect(pre?.hidden).toBe(false);
  });
});
