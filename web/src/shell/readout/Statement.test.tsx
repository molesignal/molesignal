import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import { STATEMENT_CELL, Statement } from './Statement';

describe('Statement', () => {
  afterEach(() => cleanup());

  it('says the verdict as the heading of one named surface, in the colour it is given', () => {
    render(
      <Statement
        label="Incidents"
        tone="var(--red)"
        eyebrow="Firing now"
        title="3 incidents"
        titleClassName="text-red"
        detail="2 waiting for an owner"
        dataAttributes={{ 'data-verdict': 'firing' }}
      >
        <div data-testid="spread" />
      </Statement>,
    );

    const statement = screen.getByRole('region', { name: 'Incidents' });
    expect(statement.getAttribute('data-verdict')).toBe('firing');
    expect(statement.style.getPropertyValue('--tone')).toBe('var(--red)');
    const title = screen.getByRole('heading', { level: 2, name: '3 incidents' });
    expect(title.className).toContain('text-red');
    expect(title.className).not.toContain('text-tx-0');
    // What shows how the whole divides sits with the verdict, in its cell.
    expect(title.parentElement?.contains(screen.getByTestId('spread'))).toBe(true);
    expect(title.parentElement?.textContent).toContain('Firing now');
    expect(title.parentElement?.textContent).toContain('2 waiting for an owner');
  });

  it('sets the figures beside the verdict as cells of the same surface', () => {
    render(
      <Statement
        label="Incidents"
        tone="var(--green)"
        title="All quiet"
        readoutColumns={2}
        readouts={
          <>
            <div className={STATEMENT_CELL}>One</div>
            <div className={STATEMENT_CELL}>Two</div>
          </>
        }
      />,
    );

    const statement = screen.getByRole('region', { name: 'Incidents' });
    // The surface is drawn in the hairline colour and the cells cover it, leaving the hairlines.
    expect(statement.className).toContain('bg-bd-0');
    expect(statement.className).toContain('xl:gap-px');
    const cells = screen.getByText('One').parentElement as HTMLElement;
    expect(cells.className).toContain('gap-px');
    expect(cells.className).toContain('sm:grid-cols-2');
    expect(screen.getByRole('heading', { name: 'All quiet' }).className).toContain('text-tx-0');
  });

  it('is a single cell when it has no figures beside it', () => {
    render(<Statement label="Page" tone="var(--blue)" title="Maintenance" />);

    const statement = screen.getByRole('region', { name: 'Page' });
    expect(statement.className).not.toContain('xl:grid-cols');
    expect(statement.children).toHaveLength(1);
  });
});
