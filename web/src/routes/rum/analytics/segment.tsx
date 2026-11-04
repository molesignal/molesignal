import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useSearchParams } from 'react-router-dom';

import type { RumApplication, RumFilter, RumSegment } from '@/api/rum';
import { cn } from '@/shell/lib/cn';
import { FittedSelect } from '@/shell/ui/fitted-select';

import { CONTROL } from '../overview/ScopeRow';

/** Where the address names the segment a page is narrowed to. */
export const SEGMENT_PARAM = 'segment';

const EVERYONE = '__everyone__';

/** What a segment asks of the rows: every condition, each by one of its values or by none of them. */
export function segmentFilters(segment: RumSegment | undefined): RumFilter[] {
  return (segment?.conditions ?? []).map((condition) => ({
    field: condition.field,
    op: condition.op === 'is' ? 'in' : 'not_in',
    value: JSON.stringify(condition.values),
  }));
}

/** The filters of the segment the address names, of the segments of the application. */
export function useSegmentFilters(application: RumApplication): RumFilter[] {
  const [params] = useSearchParams();
  const id = params.get(SEGMENT_PARAM);
  const segments = application.settings.segments;

  return React.useMemo(() => segmentFilters(segments?.find((segment) => segment.id === id)), [id, segments]);
}

/** Which of the groups of sessions the application names the analytics pages are of. An application without any has no choice to offer. */
export function SegmentSelect({ application }: { application: RumApplication }) {
  const { t } = useTranslation('rum');
  const [params, setParams] = useSearchParams();
  const segments = application.settings.segments ?? [];

  if (segments.length === 0) return null;

  const id = params.get(SEGMENT_PARAM);
  const current = segments.some((segment) => segment.id === id) ? (id as string) : EVERYONE;

  return (
    <FittedSelect
      aria-label={t('analytics.segment.label')}
      data-rum-segment={current}
      value={current}
      onValueChange={(value) =>
        setParams(
          (previous) => {
            const next = new URLSearchParams(previous);

            if (value === EVERYONE) next.delete(SEGMENT_PARAM);
            else next.set(SEGMENT_PARAM, value);

            return next;
          },
          { replace: true },
        )
      }
      options={[
        { value: EVERYONE, label: t('analytics.segment.everyone') },
        ...segments.map((segment) => ({ value: segment.id, label: segment.name })),
      ]}
      maxWidth={220}
      prefix={current === EVERYONE ? undefined : t('analytics.segment.label')}
      className={cn(CONTROL, 'font-strong text-tx-1')}
    />
  );
}
