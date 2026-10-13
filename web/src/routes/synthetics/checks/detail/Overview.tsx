import { useTranslation } from 'react-i18next';

import type {
  MonitorRevision,
  ProbeLocation,
  SyntheticMonitor,
  SyntheticResult,
} from '@/api/synthetics';
import { formatRelativeMicros } from '@/time/relative';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';

import { CheckStanding } from './Standing';
import { SyntheticsSection } from '../../Frame';
import { OutcomeMark, RunStrip } from '../../marks';
import { formatDuration, operationalResults, resultDurationMicros } from '../../model';
import { decidedRuns } from '../../summary';

/**
 * A check at a glance: how it stands, then how its runs have gone from each
 * place it runs. The chart and the list are the same runs, drawn and named:
 * the chart shows when a location slowed or stopped, the list says where it
 * stands now.
 */
export function CheckOverview({
  monitor,
  revision,
  results,
  locations,
  draft,
}: {
  monitor: SyntheticMonitor;
  revision: MonitorRevision;
  results: readonly SyntheticResult[];
  locations: readonly ProbeLocation[];
  /** The revision shown is a draft: it is not on schedule until it is published. */
  draft: boolean;
}) {
  const { t, i18n } = useTranslation('synthetics');
  const nowMicros = Date.now() * 1000;
  const scheduled = operationalResults([...results]);
  const places = revision.location_ids.map((id) => ({
    id,
    location: locations.find((candidate) => candidate.id === id),
    runs: scheduled
      .filter((result) => result.location_id === id)
      .sort((left, right) => right.started_at - left.started_at),
  }));
  const charted = places.filter((place) => decidedRuns(place.runs).length > 0);

  return (
    <div className="space-y-[12px]">
      <CheckStanding monitor={monitor} revision={revision} results={results} nowMicros={nowMicros} />
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
                  series={charted.map((place) => {
                    // Oldest first: the chart reads left to right.
                    const runs = decidedRuns(place.runs).reverse();

                    return {
                      id: `location-${place.id}`,
                      name: place.location?.name ?? place.id,
                      timestamps: runs.map((run) => run.started_at),
                      data: runs.map((run) => {
                        const duration = resultDurationMicros(run);
                        return duration === undefined ? null : duration / 1000;
                      }),
                      unit: 'ms',
                    };
                  })}
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
                        {latest
                          ? `${formatDuration(resultDurationMicros(latest))} · ${formatRelativeMicros(latest.started_at, i18n.language, nowMicros)}`
                          : t('detail.location_never')}
                      </div>
                    </div>
                    <RunStrip results={place.runs} length={16} className="hidden shrink-0 sm:inline-flex" />
                    <span className="w-[84px] shrink-0">
                      <OutcomeMark outcome={latest?.outcome ?? 'unknown'} />
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
