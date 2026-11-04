import { Globe2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { RumAggregate } from '@/api/rum';

import { regionName } from '../_helpers';
import { CountryMap } from '../CountryMap';
import { BarRows } from './BarRows';
import { formatCount } from './format';
import { clause, explorerPath, type OverviewScope, readCountries } from './model';
import { CardState, OverviewCard } from './OverviewCard';

/** The countries the list under the map names. */
const LISTED = 5;

/**
 * Where the sessions came from: the map of the world with each country tinted by its sessions, and
 * the commonest countries by name under it. The page leaves the card out when no session has a
 * country (an application that keeps no place, or no GeoIP database).
 */
export function OriginsCard({
  query,
  total,
  scope,
  language,
}: {
  query: { isPending: boolean; isError: boolean; error: unknown; refetch: () => unknown; data: RumAggregate | undefined };
  /** All the sessions, for the share each country has of them. */
  total: number;
  scope: OverviewScope;
  language: string;
}) {
  const { t } = useTranslation('rum');
  const countries = React.useMemo(() => readCountries(query.data), [query.data]);

  const values = React.useMemo(
    () =>
      countries.map((country) => ({
        code: country.code,
        value: country.sessions,
        name: regionName(country.code, language),
        text: t('overview.origins.value', { count: country.sessions, formatted: formatCount(country.sessions, language) }),
      })),
    [countries, language, t],
  );

  const listed = countries.slice(0, LISTED).map((country) => ({
    key: country.code,
    label: regionName(country.code, language),
    count: country.sessions,
    to: explorerPath('sessions', scope, [clause('geo.country_iso_code', country.code)]),
  }));

  const sessions = countries.map((country) => country.sessions);

  return (
    <OverviewCard
      icon={Globe2}
      tone="text-[var(--chart-7)]"
      title={t('overview.origins.title')}
      description={t('overview.origins.description')}
      testId="origins"
    >
      <CardState query={query} empty={false} emptyTitle="">
        <div className="space-y-2 pb-2 pt-3">
          <div className="px-3">
            <CountryMap
              values={values}
              ariaLabel={t('overview.origins.map')}
              legend={{
                label: t('overview.origins.legend'),
                low: formatCount(Math.min(...sessions), language),
                high: formatCount(Math.max(...sessions), language),
              }}
              language={language}
              // The card is in a page that scrolls: the wheel scrolls it, and the control key zooms the map.
              wheelZoom="modifier"
            />
          </div>
          <BarRows rows={listed} total={total} language={language} testId="countries" />
        </div>
      </CardState>
    </OverviewCard>
  );
}
