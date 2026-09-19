import { Activity, Plus } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type * as auditApi from '@/api/audit';
import { ChromeButton, Dot } from '@/shell/chrome';
import { QueryState } from '@/shell/query/State';
import type { queryStateFor } from '@/shell/query/State';
import { formatRelativeMicros } from '@/time/relative';

import { auditTarget, humanizeAction } from './format';
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
}: {
  events: auditApi.AuditEvent[];
  state: ReturnType<typeof queryStateFor>;
  error: unknown;
  onViewAll: () => void;
  onCreateAlert: () => void;
  createAlertDisabled: boolean;
  createAlertDisabledReason?: string | undefined;
}) {
  const { t, i18n } = useTranslation('onboarding');
  return (
    <CanvasSection
      className="home-canvas-detail-section"
      title={t('home.activity.title')}
      actions={<CanvasHeaderAction label={t('home.view_all')} onClick={onViewAll} />}
    >
      {state === 'empty' ? (
        <div className="m-4 grid min-h-[222px] place-items-center rounded-md bg-[var(--control-surface)] px-5 py-4 text-center">
          <div>
            <Activity className="mx-auto h-5 w-5 text-tx-3" />
            <div className="mt-2 font-sans text-sm font-strong text-tx-1">
              {t('home.activity.empty_title')}
            </div>
            <p className="mx-auto mt-1 max-w-[260px] font-sans text-xs leading-relaxed text-tx-2">
              {t('home.activity.empty_description')}
            </p>
            <ChromeButton
              size="sm"
              className="mt-3"
              disabled={createAlertDisabled}
              disabledReason={createAlertDisabledReason}
              onClick={onCreateAlert}
            >
              <Plus className="h-3 w-3" />
              {t('home.activity.create_alert')}
            </ChromeButton>
          </div>
        </div>
      ) : state ? (
        <div className="grid h-full min-h-[254px] place-items-stretch">
          <QueryState state={state} error={error} emptyLabel={t('home.activity.empty_title')} />
        </div>
      ) : (
        <ol className="relative min-h-0 flex-1 overflow-y-auto px-4 py-2">
          {events.slice(0, HOME_RECENT_ACTIVITY_LIMIT).map((event, index) => (
            <li key={event.id} className="relative flex min-h-[42px] gap-3 py-2">
              {index < Math.min(events.length, HOME_RECENT_ACTIVITY_LIMIT) - 1 && (
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
                    {formatRelativeMicros(
                      event.ts_micros,
                      i18n.resolvedLanguage ?? i18n.language,
                    )}
                  </span>
                </div>
              </div>
            </li>
          ))}
        </ol>
      )}
    </CanvasSection>
  );
}
