import { cleanup, render, screen, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';
import { DEFAULT_WINDOW, useTimeStore } from '@/stores/useTimeStore';

import { ApmPageFrame } from './components';
import { ApmFilters } from './Filters';
import { DEFAULT_APM_FILTERS } from './model';
import { ApmFilterPartContext } from './navigationSlots';

function filters() {
  return (
    <ApmFilters filters={DEFAULT_APM_FILTERS} setFilter={vi.fn()} clearFilters={vi.fn()} />
  );
}

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
  useTimeStore.setState({ window: DEFAULT_WINDOW });
});

afterEach(cleanup);

describe('APM navigation card', () => {
  it('carries the time range at the end of the tabs and the entities in a row under them', () => {
    render(
      <MemoryRouter initialEntries={['/apm/overview']}>
        <ApmPageFrame title="Application performance" subtitle="Analyze backend performance." toolbar={filters()}>
          Content
        </ApmPageFrame>
      </MemoryRouter>,
    );

    const card = document.querySelector<HTMLElement>('[data-apm-navigation="surface"]')!;
    const end = within(card.querySelector<HTMLElement>('[data-module-tabs-end]')!);
    const scope = within(card.querySelector<HTMLElement>('[data-apm-scope]')!);

    expect(end.getByRole('combobox', { name: 'Resolution' })).not.toBeNull();
    expect(end.getByRole('button', { name: 'Time range: Last 1 hour' })).not.toBeNull();
    for (const field of ['Namespace', 'Service', 'Environment', 'Version']) {
      expect(scope.getByLabelText(field)).not.toBeNull();
    }
    // Each control is on the page once, in its own place.
    expect(screen.getAllByLabelText('Namespace')).toHaveLength(1);
    expect(screen.getAllByRole('button', { name: /^Time range:/ })).toHaveLength(1);
    expect(scope.queryByRole('button', { name: /^Time range:/ })).toBeNull();
  });

  it('leaves the title row to the title and its description', () => {
    render(
      <MemoryRouter initialEntries={['/apm/overview']}>
        <ApmPageFrame title="Application performance" subtitle="Analyze backend performance." toolbar={filters()}>
          Content
        </ApmPageFrame>
      </MemoryRouter>,
    );

    const header = within(screen.getByTestId('page-header'));
    expect(header.getByRole('heading', { level: 1, name: 'Application performance' })).not.toBeNull();
    expect(header.queryByLabelText('Namespace')).toBeNull();
    expect(header.queryByRole('button', { name: /^Time range:/ })).toBeNull();
  });

  it('gives the filters a row of their own when there is no navigation to carry them', () => {
    render(
      <MemoryRouter initialEntries={['/apm/services/checkout']}>
        <ApmPageFrame title="checkout" navigation={null} toolbar={filters()}>
          Content
        </ApmPageFrame>
      </MemoryRouter>,
    );

    const row = within(document.querySelector<HTMLElement>('[data-apm-scope]')!);
    expect(row.getByLabelText('Namespace')).not.toBeNull();
    expect(row.getByRole('button', { name: 'Time range: Last 1 hour' })).not.toBeNull();
  });
});

describe('ApmFilters parts', () => {
  it('draws only the time controls, or only the entities, where a slot asks for one', () => {
    const { rerender } = render(
      <ApmFilterPartContext.Provider value="time">{filters()}</ApmFilterPartContext.Provider>,
    );
    expect(screen.getByRole('button', { name: 'Time range: Last 1 hour' })).not.toBeNull();
    expect(screen.queryByLabelText('Namespace')).toBeNull();

    rerender(
      <ApmFilterPartContext.Provider value="scope">{filters()}</ApmFilterPartContext.Provider>,
    );
    expect(screen.getByLabelText('Namespace')).not.toBeNull();
    expect(screen.queryByRole('button', { name: /^Time range:/ })).toBeNull();
    expect(screen.queryByRole('combobox', { name: 'Resolution' })).toBeNull();
  });
});
