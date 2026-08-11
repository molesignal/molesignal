import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import type { ProbeLocation, SyntheticResult } from '@/api/synthetics';
import i18n from '@/i18n';

import { WorldAvailabilityMap } from './WorldAvailabilityMap';

function location(code: string, name: string): ProbeLocation {
  return {
    id: code,
    name,
    code,
    description: '',
    scope: 'platform',
    execution: 'embedded',
    lifecycle: 'active',
    health: 'online',
    system_managed: true,
    egress_policy: {
      allowed_cidrs: [],
      denied_cidrs: [],
      allowed_domains: [],
      denied_domains: [],
      allowed_ports: [],
      allow_private_networks: false,
      allow_loopback: false,
    },
    created_at: 0,
    updated_at: 0,
  };
}

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
});

afterEach(cleanup);

describe('WorldAvailabilityMap', () => {
  it('renders interactive country geometry, availability scale, hover details, and zoom', () => {
    const { container } = render(
      <WorldAvailabilityMap
        locations={[
          location('sin', 'Singapore'),
          location('iad', 'Virginia'),
          location('local', 'Local'),
        ]}
        results={[result('sin', 'healthy'), result('iad', 'failing')]}
      />,
    );

    expect(screen.getByRole('img', { name: 'Global probe location availability map' })).toBeTruthy();
    const countries = container.querySelectorAll('[data-world-country]');
    expect(countries.length).toBeGreaterThan(150);
    expect(container.querySelectorAll('[data-world-region]')).toHaveLength(countries.length);
    expect(screen.getByText('Availability')).toBeTruthy();
    expect(screen.getByText('Unlocated · 1')).toBeTruthy();
    expect(screen.queryByText('Natural Earth')).toBeNull();

    const emptyRegion = container.querySelector(
      '[data-world-region]:not([data-world-has-locations])',
    );
    expect(emptyRegion).toBeTruthy();
    fireEvent.pointerEnter(emptyRegion as Element, { clientX: 180, clientY: 120 });
    expect(screen.getByRole('tooltip').textContent).toContain('No samples');

    const probe = container.querySelector('[data-probe-location="sin"]');
    expect(probe).toBeTruthy();
    fireEvent.pointerEnter(probe as Element, { clientX: 300, clientY: 140 });
    expect(screen.getByRole('tooltip').textContent).toContain('Singapore');
    expect(screen.getByRole('tooltip').textContent).toContain('100%');

    fireEvent.click(screen.getByRole('button', { name: 'Zoom in' }));
    expect(container.querySelector('[data-map-viewport]')?.getAttribute('transform')).toContain(
      'scale(1.5)',
    );
  });
});

function result(
  locationId: string,
  outcome: SyntheticResult['outcome'],
): SyntheticResult {
  return {
    id: `${locationId}-${outcome}`,
    organization_id: 'org-1',
    monitor_id: 'monitor-1',
    monitor_revision_id: 'revision-1',
    location_id: locationId,
    task_id: 'task-1',
    is_test: false,
    scheduled_at: 0,
    started_at: 0,
    finished_at: 1,
    received_at: 1,
    outcome,
    attempts: [],
    assertions: [],
    secret_versions: {},
    protocol_version: 1,
    metadata: {},
  };
}
