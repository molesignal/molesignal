import { useTranslation } from 'react-i18next';

import { formatMicrosActive } from '@/lib/time';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import type { Incident } from '@/types/alerting';

import { courseDuration } from './Course';
import { incidentPhases, type TimelineCategory } from './timelineModel';

/** Fired, acknowledged, resolved, each with how long after the first: what a review asks of it first. */
export function TimelinePhases({ incident }: { incident: Incident }) {
  const { t } = useTranslation('alerts');

  return (
    <dl data-incident-phases className="grid grid-cols-3 gap-3 font-sans">
      {incidentPhases(incident).map((phase) => (
        <div key={phase.key} className="min-w-0">
          <dt className="text-xs text-tx-2">{t(`incident_timeline.phases.${phase.key}`)}</dt>
          <dd className="mt-0.5 text-sm font-strong tabular-nums text-tx-0">
            {phase.after === null ? (
              <span className="font-body text-tx-3">{t('incident_timeline.phases.not_yet')}</span>
            ) : phase.key === 'fired' ? (
              formatMicrosActive(incident.createdAt)
            ) : (
              courseDuration(phase.after)
            )}
          </dd>
        </div>
      ))}
    </dl>
  );
}

export type TimelineView = 'all' | TimelineCategory;

/** Narrows the rows to one kind, with how many of each there are. Kinds with none are not offered. */
export function TimelineFilter({
  counts,
  value,
  onChange,
}: {
  counts: Record<TimelineCategory, number>;
  value: TimelineView;
  onChange: (view: TimelineView) => void;
}) {
  const { t } = useTranslation('alerts');
  const categories = (['alert', 'notice', 'people', 'notes'] as const).filter((category) => counts[category] > 0);
  const total = categories.reduce((sum, category) => sum + counts[category], 0);

  // One kind, or none: there is nothing to narrow.
  if (categories.length < 2) return null;

  return (
    <ViewTabs<TimelineView>
      label={t('incident_timeline.filter_label')}
      value={value}
      onValueChange={onChange}
      items={[
        { key: 'all', label: t('incident_timeline.filter.all'), count: total },
        ...categories.map((category) => ({
          key: category,
          label: t(`incident_timeline.filter.${category}`),
          count: counts[category],
        })),
      ]}
    />
  );
}
