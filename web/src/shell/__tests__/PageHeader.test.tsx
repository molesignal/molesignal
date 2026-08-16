import { cleanup, render, screen } from '@testing-library/react';
import { Settings } from 'lucide-react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, describe, expect, it } from 'vitest';

import { PageHeader } from '@/shell/PageHeader';

afterEach(cleanup);

describe('PageHeader module identity', () => {
  it('uses the current observation route icon and compact title spacing', () => {
    render(
      <MemoryRouter initialEntries={['/metrics']}>
        <PageHeader title="Metrics" subtitle="Explore service metrics" />
      </MemoryRouter>,
    );

    const icon = screen.getByTestId('page-header-module-icon');
    const header = screen.getByText('Metrics').closest('[class*="border-b"]');

    expect(icon.className).toContain('h-7');
    expect(icon.querySelector('svg')).not.toBeNull();
    expect(header?.className).toContain('py-1.5');
    expect(screen.getByText('·')).not.toBeNull();
    expect(screen.getByRole('heading', { level: 1, name: 'Metrics' })).not.toBeNull();
  });

  it('falls back to the owning observation module for unregistered subpages', () => {
    render(
      <MemoryRouter initialEntries={['/metrics/history']}>
        <PageHeader title="Metric history" />
      </MemoryRouter>,
    );

    expect(screen.getByTestId('page-header-module-icon')).not.toBeNull();
  });

  it('does not add module icons outside the observation navigation group', () => {
    render(
      <MemoryRouter initialEntries={['/settings/general']}>
        <PageHeader title="Settings" />
      </MemoryRouter>,
    );

    expect(screen.queryByTestId('page-header-module-icon')).toBeNull();
    expect(screen.getByRole('heading', { level: 1, name: 'Settings' })).not.toBeNull();
  });

  it('supports the Metrics-style compact layout on management pages', () => {
    render(
      <MemoryRouter initialEntries={['/settings/general']}>
        <PageHeader
          title="Settings"
          subtitle="Manage the organization"
          compact
          moduleIcon={Settings}
        />
      </MemoryRouter>,
    );

    const icon = screen.getByTestId('page-header-module-icon');
    const header = screen.getByText('Settings').closest('[class*="border-b"]');

    expect(icon.querySelector('svg')).not.toBeNull();
    expect(header?.className).toContain('py-1.5');
    expect(screen.getByText('·')).not.toBeNull();
    expect(screen.getByText('Manage the organization')).not.toBeNull();
  });
});
