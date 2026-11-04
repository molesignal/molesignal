import { useTranslation } from 'react-i18next';

import type {
  MissingRuns,
  MonitorRevision,
  ProbeLocation,
  SyntheticAvailability,
  SyntheticMonitor,
  SyntheticResult,
} from '@/api/synthetics';
import { formatRelativeMicros } from '@/time/relative';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';

import { CheckStanding } from './Standing';
import { SyntheticsSection } from '../../Frame';
import { CheckStateMark, OutcomeMark, RunStrip } from '../../marks';
import { formatDuration, formatPercent, operationalResults, resultDurationMicros } from '../../model';
import { decidedRuns } from '../../summary';

/**
 * A check at a glance: how it stands, then how its runs have gone from each
 * place it runs over the range chosen. The chart draws the range step by step,
 * and shows when a location slowed or stopped; the list says how each place
 * did over the range and where it stands now.
 */
export function CheckOverview({
  monitor,
  revision,
  results,
  availability,
  missingRuns,
  locations,
  draft,
}: {
  monitor: SyntheticMonitor;
  revision: MonitorRevision;
  results: readonly SyntheticResult[];
  /** The check's runs over the range chosen. */
  availability: SyntheticAvailability | undefined;
  /** Where the check is missing runs. */
  missingRuns: MissingRuns | undefined;
  locations: readonly ProbeLocation[];
  /** The revision shown is a draft: it is not on schedule until it is published. */
  draft: boolean;
}) {
  const { t, i18n } = useTranslation('synthetics');
  const nowMicros = Date.now() * 1000;
  const locationName = (id: string) =>
    locations.find((location) => location.id === id)?.name ?? id;
  const missed = new Map(
    (draft ? [] : (missingRuns?.locations ?? [])).map((location) => [
      location.locationId,
      location.missedRuns,
    ]),
  );
  const scheduled = operationalResults([...results]);
  const places = revision.locationIds.map((id) => ({
    id,
    location: locations.find((candidate) => candidate.id === id),
    runs: scheduled
      .filter((result) => result.locationId === id)
      .sort((left, right) => right.startedAt - left.startedAt),
  }));
  const steps = availability?.locationBuckets ?? [];
  const charted = places
    .map((place) => ({
      ...place,
      steps: steps.filter((step) => step.locationId === place.id && step.p50Ms !== null),
    }))
    .filter((place) => place.steps.length > 0);
  const overRange = new Map(
    (availability?.locations ?? []).map((location) => [location.locationId, location]),
  );

  return (
    <div className="space-y-[12px]">
      <CheckStanding
        monitor={monitor}
        revision={revision}
        results={results}
        availability={availability}
        missingRuns={draft ? undefined : missingRuns}
        locationName={locationName}
        nowMicros={nowMicros}
      />
      {draft && (
        <p role="note" className="rounded-md bg-[var(--functional-surface)] px-4 py-[10px] font-sans text-xs text-tx-1 [box-shadow:var(--shadow-functional-surface)]">
          {t('detail.draft_notice')}
        </p>
      )}

      {/* A heartbeat is reported, not run from anywhere: it has no places to compare. */}
      {places.length > 0 && (
        <div className="grid grid-cols-1 items-start gap-[12px] xl:grid-cols-[minmax(0,1.35fr)_minmax(0,1fr)]">
          <SyntheticsSection title={t('detail.response_by_location')}>
            <div className="px-4 pb-4">
              {charted.length > 0 ? (
                <TimeSeriesChart
                  height={260}
                  ariaLabel={t('detail.response_by_location')}
                  // Each step of the range at its median: one slow run does not make a slow place.
                  series={charted.map((place) => ({
                    id: `location-${place.id}`,
                    name: place.location?.name ?? place.id,
                    timestamps: place.steps.map((step) => step.start),
                    data: place.steps.map((step) => step.p50Ms),
                    unit: 'ms',
                  }))}
                  options={{
                    drawStyle: 'line',
                    showPoints: 'auto',
                    // Locations run at their own moments: without this each line is
                    // broken wherever another location has a run and it has none.
                    connectNulls: true,
                    compactAxes: true,
                    legendMode: 'list',
                    leftAxis: { min: 0, unit: 'ms' },
                  }}
                />
              ) : (
                <div className="grid h-[260px] place-items-center font-sans text-xs text-tx-3">
                  {t('states.no_results')}
                </div>
              )}
            </div>
          </SyntheticsSection>

          <SyntheticsSection title={t('detail.by_location')}>
            <ul className="divide-y divide-bd-0 border-t border-bd-0">
              {places.map((place) => {
                const latest = decidedRuns(place.runs)[0];
                const range = overRange.get(place.id);
                const missedRuns = missed.get(place.id);

                return (
                  <li
                    key={place.id}
                    data-check-location={place.id}
                    className="flex min-h-[56px] min-w-0 items-center gap-[12px] px-4 py-[8px]"
                  >
                    <div className="min-w-0 flex-1">
                      <div className="truncate font-sans text-sm font-strong text-tx-0">
                        {place.location?.name ?? place.id}
                      </div>
                      <div className="mt-[2px] truncate font-sans text-xs text-tx-2">
                        {missedRuns !== undefined
                          ? t('detail.location_missed', { count: missedRuns })
                          : latest
                          ? [
                              range?.availability !== null && range?.availability !== undefined
                                ? t('detail.location_availability', {
                                    value: formatPercent(range.availability),
                                  })
                                : undefined,
                              formatDuration(resultDurationMicros(latest)),
                              formatRelativeMicros(latest.startedAt, i18n.language, nowMicros),
                            ]
                              .filter(Boolean)
                              .join(' · ')
                          : t('detail.location_never')}
                      </div>
                    </div>
                    <RunStrip results={place.runs} length={16} className="hidden shrink-0 sm:inline-flex" />
                    <span className="w-[84px] shrink-0">
                      {missedRuns !== undefined ? (
                        <CheckStateMark state="unknown" noData />
                      ) : (
                        <OutcomeMark outcome={latest?.outcome ?? 'unknown'} />
                      )}
                    </span>
                  </li>
                );
              })}
            </ul>
          </SyntheticsSection>
        </div>
      )}
    </div>
  );
}
