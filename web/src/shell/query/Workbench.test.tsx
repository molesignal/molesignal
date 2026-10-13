import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { QueryWorkbench } from './Workbench';

const COPY = {
  mode: 'Fields',
  run: 'Run',
  summary: 'All log events',
} as const;

/** jsdom does no layout; give every element a width so the fit can be measured. */
function stubWidths({ row, item }: { row: number; item: number }) {
  Object.defineProperty(HTMLElement.prototype, 'clientWidth', {
    configurable: true,
    get() {
      return row;
    },
  });
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', {
    configurable: true,
    get() {
      return item;
    },
  });
}

function renderWorkbench(inlineBody: boolean) {
  return render(
    <QueryWorkbench
      appearance="surface"
      inlineBody={inlineBody}
      toolbar={<button type="button">{COPY.mode}</button>}
      actions={<button type="button">{COPY.run}</button>}
    >
      <div data-query-editor-state="collapsed">{COPY.summary}</div>
    </QueryWorkbench>,
  );
}

describe('QueryWorkbench', () => {
  beforeEach(() => {
    stubWidths({ row: 1600, item: 120 });
  });

  afterEach(() => {
    cleanup();
    delete (HTMLElement.prototype as { clientWidth?: number }).clientWidth;
    delete (HTMLElement.prototype as { offsetWidth?: number }).offsetWidth;
  });

  it('keeps an open editor in its own row under the toolbar', () => {
    const { container } = renderWorkbench(false);

    const section = container.querySelector('[data-query-workbench-appearance]');
    expect(section?.getAttribute('data-query-body')).toBe('stacked');
    expect(container.querySelector('[data-query-inline-body]')).toBeNull();
    // The body follows the toolbar row instead of sitting inside it.
    const toolbar = container.querySelector('[data-query-toolbar]');
    expect(toolbar?.contains(screen.getByText(COPY.summary))).toBe(false);
  });

  it('seats a one-line body in the toolbar row, between the controls and the actions', () => {
    const { container } = renderWorkbench(true);

    const section = container.querySelector('[data-query-workbench-appearance]');
    expect(section?.getAttribute('data-query-body')).toBe('inline');

    const toolbar = container.querySelector('[data-query-toolbar]')!;
    const slots = Array.from(toolbar.children);
    expect(slots).toHaveLength(3);
    expect(slots[0]?.contains(screen.getByRole('button', { name: COPY.mode }))).toBe(true);
    expect(slots[1]?.getAttribute('data-query-inline-body')).not.toBeNull();
    expect(slots[1]?.contains(screen.getByText(COPY.summary))).toBe(true);
    expect(slots[2]?.contains(screen.getByRole('button', { name: COPY.run }))).toBe(true);
    // The collapsed editor takes the height and corner of the controls beside it.
    expect(slots[1]?.className).toContain('[&_[data-query-editor-state=collapsed]>button]:h-9');
  });

  it('leaves the body below when the row has no room for it', () => {
    // Controls 120 + actions 120 + gaps leave less than a readable summary.
    stubWidths({ row: 500, item: 120 });
    const { container } = renderWorkbench(true);

    const section = container.querySelector('[data-query-workbench-appearance]');
    expect(section?.getAttribute('data-query-body')).toBe('stacked');
    expect(container.querySelector('[data-query-inline-body]')).toBeNull();
    expect(screen.getByText(COPY.summary)).not.toBeNull();
  });

  it.each([
    ['inline', 1600],
    ['stacked', 500],
  ] as const)('gives a collapsed editor its pointer and keyboard fill while the body is %s', (body, row) => {
    stubWidths({ row, item: 120 });
    const { container } = renderWorkbench(true);

    const section = container.querySelector('[data-query-workbench-appearance]');
    expect(section?.getAttribute('data-query-body')).toBe(body);
    // In the toolbar row or under it, the editor is inside what carries the two fills.
    const editor = container.querySelector('[data-query-editor-state="collapsed"]');
    for (const state of ['hover', 'focus-visible']) {
      const fill = `[&_[data-query-editor-state=collapsed]>button:${state}]:bg-bg-3`;
      expect(editor?.closest(`[class~="${fill}"]`), fill).not.toBeNull();
    }
  });
});
