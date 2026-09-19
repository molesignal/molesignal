import { cleanup, render, screen, within } from '@testing-library/react';
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
        <PageHeader title="Settings" subtitle="Manage the organization" />
      </MemoryRouter>,
    );

    expect(screen.queryByTestId('page-header-module-icon')).toBeNull();
    expect(screen.getByTestId('page-header')).toHaveAttribute(
      'data-page-header-layout',
      'inline',
    );
    expect(screen.getByText('·')).not.toBeNull();
    expect(screen.getByText('Manage the organization')).not.toBeNull();
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

describe('PageHeader navigation hierarchy', () => {
  it('does not repeat same-level RUM navigation above the module tabs', () => {
    render(
      <MemoryRouter initialEntries={['/rum/overview']}>
        <PageHeader title="User Experience" />
      </MemoryRouter>,
    );

    expect(screen.queryByRole('navigation', { name: 'Breadcrumb' })).toBeNull();
    expect(screen.queryByRole('link', { name: 'Back' })).toBeNull();
  });

  it('shows only the module-internal drill-down path and suppresses Back', () => {
    render(
      <MemoryRouter initialEntries={['/rum/sessions/view/session-42']}>
        <PageHeader title="Session 42" />
      </MemoryRouter>,
    );

    const breadcrumb = screen.getByRole('navigation', { name: 'Breadcrumb' });
    expect(within(breadcrumb).getByRole('link').getAttribute('href')).toBe('/rum/sessions');
    expect(breadcrumb.querySelector('[aria-current="page"]')).not.toBeNull();
    expect(breadcrumb.querySelector('a[href="/rum/overview"]')).toBeNull();
    expect(screen.queryByRole('link', { name: 'Back' })).toBeNull();
  });

  it('keeps a two-part resource path when the module is also the collection', () => {
    render(
      <MemoryRouter initialEntries={['/profiles/profile-42']}>
        <PageHeader title="Profile 42" />
      </MemoryRouter>,
    );

    const breadcrumb = screen.getByRole('navigation', { name: 'Breadcrumb' });
    expect(within(breadcrumb).getByRole('link').getAttribute('href')).toBe('/profiles');
    expect(breadcrumb.querySelector('[aria-current="page"]')).not.toBeNull();
    expect(screen.queryByRole('link', { name: 'Back' })).toBeNull();
  });

  it('allows an isolated workspace to opt into a standalone Back link', () => {
    render(
      <MemoryRouter initialEntries={['/dashboards/dashboard-42']}>
        <PageHeader
          title="Dashboard 42"
          breadcrumbs={null}
          backTo="/dashboards"
        />
      </MemoryRouter>,
    );

    expect(screen.getByRole('link', { name: 'Back' })).toBeTruthy();
    expect(screen.queryByRole('navigation', { name: 'Breadcrumb' })).toBeNull();
  });
});
