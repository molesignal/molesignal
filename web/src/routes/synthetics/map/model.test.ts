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
    systemManaged: true,
    egressPolicy: {
      allowedCidrs: [],
      deniedCidrs: [],
      allowedDomains: [],
      deniedDomains: [],
      allowedPorts: [],
      allowPrivateNetworks: false,
      allowLoopback: false,
    },
    createdAt: 0,
    updatedAt: 0,
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
    organizationId: 'org-1',
    monitorId: 'monitor-1',
    monitorRevisionId: 'revision-1',
    locationId: locationId,
    taskId: 'task-1',
    isTest: isTest,
    scheduledAt: 0,
    startedAt: 0,
    finishedAt: 1,
    receivedAt: 1,
    outcome,
    attempts: [],
    assertions: [],
    secretVersions: {},
    protocolVersion: 1,
    metadata: {},
  };
}
