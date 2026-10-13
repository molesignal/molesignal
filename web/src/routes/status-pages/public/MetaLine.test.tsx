import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import { MetaLine } from './MetaLine';

describe('MetaLine', () => {
  afterEach(() => cleanup());

  it('sets each fact as one item, and keeps a fragment of text and a time together', () => {
    render(
      <MetaLine>
        <>Major impact</>
        <>
          Resolved <time dateTime="2026-08-04T21:00:00.000Z">9:00 PM</time>
        </>
        {false}
        <span data-testid="last">API</span>
      </MetaLine>,
    );

    const line = screen.getByText('Major impact').parentElement?.parentElement as HTMLElement;
    // Three facts: the one that was not there leaves no item, and so no dot, behind.
    expect(line.children).toHaveLength(3);
    expect(line.children[1]?.textContent).toBe('Resolved 9:00 PM');
    expect(line.children[2]?.contains(screen.getByTestId('last'))).toBe(true);
  });

  it('draws the dots as part of the facts, so none of them is read out or copied as text', () => {
    render(
      <MetaLine>
        <>One</>
        <>Two</>
      </MetaLine>,
    );

    expect(screen.getByText('One').closest('span[class*="overflow-hidden"]')?.textContent).toBe('OneTwo');
  });
});
