import '@/i18n';

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';
import { TooltipProvider } from '@/shell/ui/tooltip';
import { Attributes } from '@/viz/trace/span-detail/Attributes';
import { AttributeFilterContext } from '@/viz/trace/span-detail/model';

afterEach(cleanup);
const label = (key: string) => i18n.t(`attributes.${key}`, { ns: 'traces' });
function setup(attributes: Record<string, unknown>, filter = vi.fn()) {
  render(<MemoryRouter><TooltipProvider><AttributeFilterContext.Provider value={filter}>
    <Attributes attributes={attributes} />
  </AttributeFilterContext.Provider></TooltipProvider></MemoryRouter>);
  return filter;
}

describe('span attributes', () => {
  it('searches values outside the initial eight and expands all fields', async () => {
    const user = userEvent.setup();
    setup(Object.fromEntries(Array.from({ length: 12 }, (_, i) => [`tag.${String(i).padStart(2, '0')}`, `value-${i}`])));
    expect(screen.queryByText('tag.11')).not.toBeInTheDocument();
    await user.type(screen.getByRole('textbox', { name: label('search') }), 'value-11');
    expect(screen.getByText('tag.11')).toBeVisible();
    expect(screen.queryByText('tag.00')).not.toBeInTheDocument();
    await user.clear(screen.getByRole('textbox', { name: label('search') }));
    await user.click(screen.getByRole('button', { name: i18n.t('attributes.show_all', { ns: 'traces', count: 12 }) }));
    expect(screen.getByText('tag.11')).toBeVisible();
  });

  it('preserves nested values in raw JSON mode', async () => {
    const user = userEvent.setup();
    const attributes = { 'db.statement': { query: 'SELECT "a"', count: 2 } };
    setup(attributes);
    await user.click(screen.getByRole('button', { name: label('json') }));
    expect(JSON.parse(document.querySelector('pre')!.textContent!)).toEqual(attributes);
  });

  it('adds a typed exclusion through the query callback', async () => {
    const user = userEvent.setup();
    const filter = setup({ 'http.status_code': 500 });
    const menu = screen.getByRole('button', { name: i18n.t('attributes.actions', { ns: 'traces', key: 'http.status_code' }) });
    fireEvent.keyDown(menu, { key: 'Enter' });
    await user.click(await screen.findByRole('menuitem', { name: label('exclude') }));
    expect(filter).toHaveBeenCalledWith('http.status_code', 500, '!=');
  });
});
