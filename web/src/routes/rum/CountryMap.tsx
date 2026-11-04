import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { MAP_HEIGHT, MAP_WIDTH } from '@/viz/worldmap/size';
import type { MapLegend, MapValue } from '@/viz/worldmap/ValueWorldMap';

import { regionName } from './_helpers';

// The shapes of the world are the larger part of the map: they come when a map is first drawn,
// or sooner where the page asks for them (`usePreloadWorldMap`).
const ValueWorldMap = React.lazy(() => import('@/viz/worldmap/ValueWorldMap').then((module) => ({ default: module.ValueWorldMap })));

/**
 * The countries of some rows on the map of the world, each tinted by its number, with the zoom of
 * a map. The map is loaded when it is first drawn, and holds its room until then.
 */
export function CountryMap({
  values,
  ariaLabel,
  legend,
  language,
  wheelZoom,
  className,
}: {
  values: readonly MapValue[];
  ariaLabel: string;
  legend?: MapLegend | undefined;
  language: string;
  /** `modifier` leaves the plain wheel to the page: the card of a page that scrolls. */
  wheelZoom?: 'always' | 'modifier';
  className?: string;
}) {
  const { t } = useTranslation('rum');

  const labels = React.useMemo(
    () => ({
      zoomIn: t('map.zoom_in'),
      zoomOut: t('map.zoom_out'),
      reset: t('map.zoom_reset'),
      zoomLevel: (zoom: number) => t('map.zoom_level', { level: zoom.toFixed(1) }),
      noData: t('map.no_data'),
      keys: t('map.keys'),
      wheelHint: t('map.wheel_hint'),
    }),
    [t],
  );
  const nameOf = React.useCallback((code: string | null, english: string) => (code ? regionName(code, language) : english), [language]);

  return (
    <React.Suspense
      fallback={<div aria-hidden className="w-full rounded-md bg-bg-2" style={{ aspectRatio: `${MAP_WIDTH} / ${MAP_HEIGHT}` }} />}
    >
      <ValueWorldMap
        values={values}
        ariaLabel={ariaLabel}
        labels={labels}
        legend={legend}
        nameOf={nameOf}
        {...(wheelZoom ? { wheelZoom } : {})}
        {...(className ? { className } : {})}
      />
    </React.Suspense>
  );
}
