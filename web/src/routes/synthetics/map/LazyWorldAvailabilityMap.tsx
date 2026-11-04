import * as React from 'react';

import type { AvailabilitySummary, ProbeLocation, SyntheticResult } from '@/api/synthetics';

// The shapes of the world are the larger part of the map, and a tab without a check has no use
// for them: they come when the map is first drawn, or sooner where the tab asks (`usePreloadWorldMap`).
const WorldAvailabilityMap = React.lazy(() =>
  import('./WorldAvailabilityMap').then((module) => ({ default: module.WorldAvailabilityMap })),
);

/** The map of availability by probe location, loaded when it is first drawn; its room is held until then. */
export function LazyWorldAvailabilityMap({
  locations,
  results,
  availability,
}: {
  locations: ProbeLocation[];
  results?: SyntheticResult[];
  availability?: ReadonlyArray<AvailabilitySummary & { locationId: string }>;
}) {
  return (
    <React.Suspense fallback={<div aria-hidden className="h-[310px] bg-bg-2 sm:h-[360px]" />}>
      <WorldAvailabilityMap
        locations={locations}
        {...(results ? { results } : {})}
        {...(availability ? { availability } : {})}
      />
    </React.Suspense>
  );
}
