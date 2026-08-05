import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';
import { DEFAULT_WINDOW, useTimeStore } from '@/stores/useTimeStore';

import { ApmFilters } from './Filters';
import { DEFAULT_APM_FILTERS } from './model';

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
  useTimeStore.setState({ window: DEFAULT_WINDOW });
});

afterEach(() => {
  cleanup();
  useTimeStore.setState({ window: DEFAULT_WINDOW });
});

describe('ApmFilters', () => {
  it('exposes the shared time-range picker and updates the APM query window', () => {
    render(
      <ApmFilters
        filters={DEFAULT_APM_FILTERS}
        setFilter={vi.fn()}
        clearFilters={vi.fn()}
      />,
    );

    fireEvent.click(
      screen.getByRole('button', { name: 'Time range: Last 1 hour' }),
    );
    fireEvent.click(screen.getByRole('button', { name: 'Last 6 hours' }));

    expect(useTimeStore.getState().window).toEqual({
      from: 'now-6h',
      to: 'now',
      mode: 'relative',
    });
  });
});
