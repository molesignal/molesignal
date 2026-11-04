import { useTranslation } from 'react-i18next';

import { FittedSelect } from '@/shell/ui/fitted-select';
import { TimeRangeControl } from '@/time/TimePicker';

import type { ApmScope } from '../scope';

// `sm:h-8` too: a select's trigger is 32px from the small width up unless told otherwise, and
// these three stand in one row.
const CONTROL = 'h-8 border-0 bg-[var(--control-surface)] px-2.5 text-xs hover:bg-bg-3 focus-visible:bg-bg-3 data-[state=open]:bg-bg-3 sm:h-8';

/**
 * The end of an APM navigation row: how finely the time is bucketed, the
 * period drawn under the current one where the page has charts, and the
 * time range with its zone.
 */
export function ApmTimeControls({
  scope,
  onScopeChange,
  comparable = false,
  environments = [],
}: {
  scope: ApmScope;
  onScopeChange: (patch: Partial<ApmScope>) => void;
  /** The page draws charts a comparison can go under. */
  comparable?: boolean;
  /** Other environments the figures can be compared with: a service's own. */
  environments?: readonly string[];
}) {
  const { t } = useTranslation('apm');

  return (
    <div className="flex items-center gap-2">
      {comparable && (
        <FittedSelect
          data-apm-compare
          aria-label={t('time.compare')}
          value={scope.compare}
          onValueChange={(compare) => onScopeChange({ compare: compare as ApmScope['compare'] })}
          options={[
            { value: 'off', label: t('time.compare_off') },
            { value: 'day', label: t('time.compare_day') },
            { value: 'week', label: t('time.compare_week') },
            ...environments.map((environment) => ({ value: `env:${environment}`, label: t('time.compare_env', { environment }) })),
          ]}
          maxWidth={200}
          className={CONTROL}
        />
      )}
      <FittedSelect
        data-apm-resolution
        aria-label={t('time.resolution')}
        value={scope.resolution}
        onValueChange={(resolution) => onScopeChange({ resolution: resolution as ApmScope['resolution'] })}
        options={[
          { value: 'auto', label: t('time.resolution_auto') },
          { value: 'minute', label: t('time.resolution_minute') },
          { value: 'hour', label: t('time.resolution_hour') },
        ]}
        maxWidth={160}
        className={CONTROL}
      />
      <TimeRangeControl align="end" className={`${CONTROL} max-w-[220px]`} />
    </div>
  );
}
