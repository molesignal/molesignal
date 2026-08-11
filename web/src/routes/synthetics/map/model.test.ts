import { describe, expect, it } from 'vitest';

import type { ProbeLocation, SyntheticResult } from '@/api/synthetics';

import {
  availabilityHistogram,
  partitionProbeLocations,
  probeLocationCoordinates,
  summarizeProbeLocationAvailability,
} from './model';

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

describe('probeLocationCoordinates', () => {
  it('maps known public probe codes to real longitude and latitude', () => {
    expect(probeLocationCoordinates(location('sin', 'Singapore'))).toEqual([103.8198, 1.3521]);
    expect(probeLocationCoordinates(location('iad', 'Virginia'))).toEqual([-78.6569, 37.4316]);
  });

  it('does not invent a point for a private or local probe without coordinates', () => {
    expect(probeLocationCoordinates(location('local', 'Local'))).toBeUndefined();
  });

  it('keeps unlocated probes separate from projected markers', () => {
    const locations = [location('fra', 'Frankfurt'), location('private-a', 'Warehouse')];

    const result = partitionProbeLocations(locations);

    expect(result.mapped.map(({ location: item }) => item.code)).toEqual(['fra']);
    expect(result.unmapped.map((item) => item.code)).toEqual(['private-a']);
  });

  it('calculates per-location availability from operational decided results', () => {
    const locations = [location('sin', 'Singapore')];
    const results = [
      result('sin', 'healthy'),
      result('sin', 'healthy'),
      result('sin', 'failing'),
      result('sin', 'healthy', true),
    ];

    const summary = summarizeProbeLocationAvailability(locations, results).mapped[0];

    expect(summary).toMatchObject({ samples: 3, availability: 2 / 3 });
  });

  it('builds bounded histogram bins for the Cloudflare-style scale', () => {
    expect(availabilityHistogram([-1, 0.5, 1, 2], 4)).toEqual([1, 0, 1, 2]);
  });
});

function result(
  locationId: string,
  outcome: SyntheticResult['outcome'],
  isTest = false,
): SyntheticResult {
  return {
    id: `${locationId}-${outcome}-${isTest}`,
    organization_id: 'org-1',
    monitor_id: 'monitor-1',
    monitor_revision_id: 'revision-1',
    location_id: locationId,
    task_id: 'task-1',
    is_test: isTest,
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
