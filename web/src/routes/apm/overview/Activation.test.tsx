import { cleanup, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';

import { QueryBoundary } from '../components';
import { ApmActivation } from './Activation';

const COPY = { content: 'Aggregates' } as const;

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
});

afterEach(cleanup);

function renderBoundary(props: { empty: boolean; filtered: boolean }) {
  return render(
    <MemoryRouter>
      <QueryBoundary
        pending={false}
        error={null}
        onRetry={vi.fn()}
        emptyState={<ApmActivation />}
        {...props}
      >
        <p>{COPY.content}</p>
      </QueryBoundary>
    </MemoryRouter>,
  );
}

describe('APM overview with no data', () => {
  it('says how to start sending traces when nothing has arrived yet', () => {
    renderBoundary({ empty: true, filtered: false });

    expect(screen.getByRole('heading', { level: 2, name: 'No APM data yet' })).not.toBeNull();
    expect(screen.getByRole('link', { name: 'Connect a service' }).getAttribute('href')).toBe(
      '/datasource/applications/opentelemetry?signal=traces',
    );
    expect(screen.getByRole('link', { name: 'Open Traces' }).getAttribute('href')).toBe('/traces');
    expect(screen.getAllByRole('listitem').map((step) => step.textContent?.slice(0, 2))).toEqual([
      '1I',
      '2S',
      '3W',
    ]);
    expect(screen.queryByText(COPY.content)).toBeNull();
  });

  it('says the filters hide everything when it is the filters that do', () => {
    renderBoundary({ empty: true, filtered: true });

    expect(screen.getByText('No results match these filters')).not.toBeNull();
    expect(screen.queryByTestId('apm-activation')).toBeNull();
  });

  it('shows the page once there is data', () => {
    renderBoundary({ empty: false, filtered: false });

    expect(screen.getByText(COPY.content)).not.toBeNull();
    expect(screen.queryByTestId('apm-activation')).toBeNull();
  });
});
