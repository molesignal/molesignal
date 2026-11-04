import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { RumAggregate } from '@/api/rum';

import { countryValues, formatMeasure } from './answer';
import { regionName } from '../../_helpers';
import { CountryMap } from '../../CountryMap';

/**
 * The countries on the map of the world, each tinted by its number next to the largest, and the
 * map zooms. A row with no country has no place on it, and a type with no country at all has an
 * empty map, which says why.
 */
export function RumVizMap({
  answer,
  title,
  measureName,
  language,
}: {
  answer: RumAggregate;
  title: string;
  /** What the colour is of. */
  measureName: string;
  language: string;
}) {
  const { t } = useTranslation('rum');
  const unit = answer.measures[0]?.unit ?? 'none';
  const countries = React.useMemo(() => countryValues(answer), [answer]);
  const values = React.useMemo(
    () =>
      countries.map((country) => ({
        code: country.code,
        value: country.value,
        name: regionName(country.code, language),
        text: formatMeasure(country.value, unit, language),
      })),
    [countries, language, unit],
  );
  const numbers = countries.map((country) => country.value);

  return (
    <div data-rum-viz-body="map" role="region" aria-label={title} tabIndex={0} className="min-h-0 flex-1 overflow-auto p-3 focus-visible:outline-none">
      <h2 className="mb-2 font-sans text-sm font-strong text-tx-0">{title}</h2>
      {countries.length === 0 ? (
        <p className="font-sans text-xs text-tx-3">{t('explorer.viz.map_empty')}</p>
      ) : (
        <CountryMap
          values={values}
          ariaLabel={title}
          legend={{ label: measureName, low: formatMeasure(Math.min(...numbers), unit, language), high: formatMeasure(Math.max(...numbers), unit, language) }}
          language={language}
        />
      )}
    </div>
  );
}
