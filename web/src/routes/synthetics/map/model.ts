import type { LocationHealth, ProbeLocation, SyntheticResult } from '@/api/synthetics';

export type GeographicCoordinates = readonly [longitude: number, latitude: number];

export interface MappedProbeLocation {
  location: ProbeLocation;
  coordinates: GeographicCoordinates;
}

export interface ProbeLocationAvailability extends MappedProbeLocation {
  availability: number | undefined;
  samples: number;
}

interface KnownLocation {
  coordinates: GeographicCoordinates;
  codes: readonly string[];
  names: readonly string[];
}

const KNOWN_LOCATIONS: readonly KnownLocation[] = [
  {
    coordinates: [103.8198, 1.3521],
    codes: ['sg', 'sin', 'singapore'],
    names: ['singapore', '新加坡'],
  },
  {
    coordinates: [139.6503, 35.6762],
    codes: ['jp', 'tyo', 'tokyo'],
    names: ['tokyo', '东京', '東京'],
  },
  {
    coordinates: [114.1694, 22.3193],
    codes: ['hk', 'hkg', 'hong-kong'],
    names: ['hong kong', 'hongkong', '香港'],
  },
  {
    coordinates: [151.2093, -33.8688],
    codes: ['au', 'syd', 'sydney'],
    names: ['sydney', '悉尼'],
  },
  {
    coordinates: [8.6821, 50.1109],
    codes: ['de', 'fra', 'frankfurt'],
    names: ['frankfurt', '法兰克福', '法蘭克福'],
  },
  {
    coordinates: [-78.6569, 37.4316],
    codes: ['iad', 'us-east', 'us-east-1', 'virginia'],
    names: ['virginia', '弗吉尼亚', '維吉尼亞'],
  },
] as const;

const HEALTH_COLOR: Record<LocationHealth, string> = {
  online: 'var(--green)',
  degraded: 'var(--yellow)',
  offline: 'var(--red)',
  unknown: 'var(--tx-3)',
};

export function probeLocationCoordinates(
  location: Pick<ProbeLocation, 'code' | 'name'>,
): GeographicCoordinates | undefined {
  const code = normalizeCode(location.code);
  const name = normalizeName(location.name);
  return KNOWN_LOCATIONS.find(
    (known) => known.codes.includes(code) || known.names.some((alias) => name.includes(alias)),
  )?.coordinates;
}

export function partitionProbeLocations(locations: ProbeLocation[]): {
  mapped: MappedProbeLocation[];
  unmapped: ProbeLocation[];
} {
  const mapped: MappedProbeLocation[] = [];
  const unmapped: ProbeLocation[] = [];

  locations.forEach((location) => {
    const coordinates = probeLocationCoordinates(location);
    if (coordinates) mapped.push({ location, coordinates });
    else unmapped.push(location);
  });

  return { mapped, unmapped };
}

export function summarizeProbeLocationAvailability(
  locations: ProbeLocation[],
  results: SyntheticResult[],
): { mapped: ProbeLocationAvailability[]; unmapped: ProbeLocation[] } {
  const { mapped, unmapped } = partitionProbeLocations(locations);
  const decidedByLocation = new Map<string, SyntheticResult[]>();

  results.forEach((result) => {
    if (result.is_test || result.outcome === 'unknown' || result.outcome === 'skipped') return;
    const decided = decidedByLocation.get(result.location_id) ?? [];
    decided.push(result);
    decidedByLocation.set(result.location_id, decided);
  });

  return {
    mapped: mapped.map(({ location, coordinates }) => {
      const decided = decidedByLocation.get(location.id) ?? [];
      return {
        location,
        coordinates,
        samples: decided.length,
        availability:
          decided.length > 0
            ? decided.filter((result) => result.outcome === 'healthy').length / decided.length
            : undefined,
      };
    }),
    unmapped,
  };
}

export function availabilityColor(value: number | undefined): string {
  if (value === undefined) return 'var(--bg-3)';
  const percentage = Math.round(Math.min(1, Math.max(0, value)) * 100);
  return `color-mix(in srgb, var(--indigo) ${18 + Math.round(percentage * 0.82)}%, var(--bg-3))`;
}

export function availabilityHistogram(values: number[], binCount = 20): number[] {
  const bins = Array.from({ length: binCount }, () => 0);
  values.forEach((value) => {
    if (!Number.isFinite(value)) return;
    const normalized = Math.min(1, Math.max(0, value));
    const index = Math.min(binCount - 1, Math.floor(normalized * binCount));
    bins[index] = (bins[index] ?? 0) + 1;
  });
  return bins;
}

export function probeHealthColor(health: LocationHealth): string {
  return HEALTH_COLOR[health];
}

function normalizeCode(value: string): string {
  return value.trim().toLocaleLowerCase().replaceAll('_', '-');
}

function normalizeName(value: string): string {
  return value.trim().toLocaleLowerCase().replace(/[\s_-]+/g, ' ');
}
