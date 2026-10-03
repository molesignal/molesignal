import { Activity } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type * as auditApi from '@/api/audit';
import { Dot } from '@/shell/chrome';
import { EmptyIllustration } from '@/shell/EmptyIllustration';
import { EmptyState } from '@/shell/EmptyState';
import { cn } from '@/shell/lib/cn';
import { QueryState } from '@/shell/query/State';
import type { queryStateFor } from '@/shell/query/State';
import { formatRelativeMicros } from '@/time/relative';

import { auditTarget, humanizeAction } from './format';
import { ListSkeleton } from './HomeSkeletons';
import { CanvasHeaderAction, CanvasSection } from './layout';

export const HOME_RECENT_ACTIVITY_LIMIT = 8;

export function RecentActivitySection({
  events,
  state,
  error,
  onViewAll,
  onCreateAlert,
  createAlertDisabled,
  createAlertDisabledReason,
  riseIndex,
}: {
  events: auditApi.AuditEvent[];
  state: ReturnType<typeof queryStateFor>;
  error: unknown;
  onViewAll: () => void;
  onCreateAlert: () => void;
  createAlertDisabled: boolean;
  createAlertDisabledReason?: string | undefined;
  riseIndex?: number;
}) {
  const { t, i18n } = useTranslation('onboarding');

  const body = () => {
    if (state === 'loading') return <ListSkeleton />;
    if (state === 'error') {
      return (
        <div className="grid min-h-[180px] place-items-stretch">
          <QueryState state="error" error={error} />
        </div>
      );
    }
    if (state === 'empty') {
      return (
        <EmptyState
          size="compact"
          icon={Activity}
          illustration={<EmptyIllustration kind="activity" />}
          title={t('home.activity.empty_title')}
          description={t('home.activity.empty_description')}
          primaryAction={{
            label: t('home.activity.create_alert'),
            onClick: onCreateAlert,
            disabled: createAlertDisabled,
            disabledReason: createAlertDisabledReason,
          }}
        />
      );
    }
    return (
      <ActivityList
        events={events.slice(0, HOME_RECENT_ACTIVITY_LIMIT)}
        locale={i18n.resolvedLanguage ?? i18n.language}
      />
    );
  };

  return (
    <CanvasSection
      className="home-canvas-activity-section"
      title={t('home.activity.title')}
      actions={<CanvasHeaderAction label={t('home.view_all')} onClick={onViewAll} />}
      {...(riseIndex !== undefined ? { riseIndex } : {})}
    >
      {body()}
    </CanvasSection>
  );
}

function ActivityList({
  events,
  locale,
}: {
  events: auditApi.AuditEvent[];
  locale: string;
}) {
  const { viewportRef, fitted } = useFittedRows(events.length);
  return (
    <div ref={viewportRef} className="min-h-0 flex-1 overflow-hidden">
      <ol className="relative px-[20px] pb-3 pt-1">
        {events.map((event, index) => (
          <li
            key={event.id}
            data-fit-row
            // Rows that do not fit stay in the flow but are hidden, so nothing
            // is ever cut in half and a taller column brings them back.
            className={cn('relative flex min-h-[42px] gap-3 py-2', index >= fitted && 'invisible')}
          >
            {index < fitted - 1 && (
              <span
                aria-hidden="true"
                className="pointer-events-none absolute inset-y-0 left-0 w-1.5"
              >
                <span className="absolute -bottom-3 left-1/2 top-[1.375rem] w-px -translate-x-1/2 bg-bd-1" />
              </span>
            )}
            <Dot tone="indigo" className="relative z-10 mt-1.5 ring-2 ring-bg-1" />
            <div className="min-w-0 flex-1">
              <div className="truncate font-sans text-xs font-strong text-tx-0">
                {humanizeAction(event.action)}
              </div>
              <div className="mt-0.5 flex min-w-0 items-center gap-2 font-sans text-xs text-tx-2">
                <span className="min-w-0 flex-1 truncate">{auditTarget(event)}</span>
                <span className="shrink-0 text-tx-3">
                  {formatRelativeMicros(event.ts_micros, locale)}
                </span>
              </div>
            </div>
          </li>
        ))}
      </ol>
    </div>
  );
}

/**
 * How many of the `[data-fit-row]` rows sit fully inside the viewport. In the
 * wide layout the card is as tall as the column beside it, so the list shows
 * what fits rather than whatever happens to be returned.
 */
function useFittedRows(total: number) {
  const viewportRef = React.useRef<HTMLDivElement>(null);
  const [fitted, setFitted] = React.useState(total);

  React.useLayoutEffect(() => {
    const viewport = viewportRef.current;
    if (!viewport) return;
    const measure = () => {
      const bottom = viewport.getBoundingClientRect().bottom + 0.5;
      const rows = Array.from(viewport.querySelectorAll<HTMLElement>('[data-fit-row]'));
      const overflowAt = rows.findIndex((row) => row.getBoundingClientRect().bottom > bottom);
      setFitted(overflowAt === -1 ? rows.length : Math.max(1, overflowAt));
    };
    measure();
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(measure);
    observer.observe(viewport);
    const list = viewport.firstElementChild;
    if (list) observer.observe(list);
    return () => observer.disconnect();
  }, [total]);

  return { viewportRef, fitted };
}
