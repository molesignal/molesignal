import { useQuery } from '@tanstack/react-query';
import { Settings2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as postmortemsApi from '@/api/postmortems';
import type { PostmortemSummary } from '@/api/postmortems';
import { formatMicrosActive } from '@/lib/time';
import { useActionAccess } from '@/product/actionAccess';
import type { ProductStateProps } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { PostmortemDrawer } from '@/shell/incident/PostmortemDrawer';
import { cn } from '@/shell/lib/cn';
import { useUsers } from '@/shell/useUsers';

import { AlertsPage } from '../Frame';
import { ResponseSettingsDrawer } from './ResponseSettingsDrawer';

const STATUS_INK: Record<PostmortemSummary['status'], string> = {
  draft: 'text-tx-2',
  in_review: 'text-orange',
  published: 'text-green',
};

/** Internal postmortems, their status and the action items still open or late; and how major incidents are run. */
export function AlertsPostmortems() {
  const { t } = useTranslation('alerts');
  const { nameOf } = useUsers();
  const manageAccess = useActionAccess({ permission: 'alerts.response.manage' });
  const [opened, setOpened] = React.useState<PostmortemSummary | null>(null);
  const [settingsOpen, setSettingsOpen] = React.useState(false);
  const postmortems = useQuery({ queryKey: ['postmortems'], queryFn: postmortemsApi.list });
  const state: ProductStateProps | null = postmortems.isLoading
    ? { variant: 'loading' }
    : postmortems.isError
      ? { variant: 'error', error: postmortems.error }
      : null;

  return (
    <>
      <AlertsPage
        action={
          <ChromeButton disabled={manageAccess.disabled} disabledReason={manageAccess.reason} onClick={() => setSettingsOpen(true)}>
            <Settings2 className="h-4 w-4" />
            {t('major.settings.open')}
          </ChromeButton>
        }
        state={state}
        contentSurface={false}
      >
        {(postmortems.data ?? []).length === 0 ? (
          <p className="bg-[var(--functional-surface)] px-[12px] py-4 font-sans text-sm text-tx-2">{t('postmortems.empty')}</p>
        ) : (
          <ul data-postmortems className="flex flex-col gap-px">
            {(postmortems.data ?? []).map((item) => (
              <li
                key={item.id}
                data-postmortem={item.id}
                className="flex flex-wrap items-center gap-x-3 gap-y-1 bg-[var(--functional-surface)] py-[10px] pl-[12px] pr-[20px] font-sans"
              >
                <div className="min-w-[240px] flex-1">
                  <button
                    type="button"
                    onClick={() => setOpened(item)}
                    className="block max-w-full truncate text-left text-sm font-strong text-tx-0 underline-offset-4 hover:underline"
                  >
                    {item.title}
                  </button>
                  <Link
                    to={`/alerts/incidents/${encodeURIComponent(item.incidentId)}`}
                    className="mt-0.5 block truncate text-xs text-tx-2 underline-offset-2 hover:underline"
                  >
                    {item.incidentSummary || item.incidentId}
                  </Link>
                </div>
                <span className={cn('w-[80px] shrink-0 text-xs font-strong', STATUS_INK[item.status])}>
                  {t(`postmortems.status.${item.status}`)}
                </span>
                <span className="w-[120px] shrink-0 truncate text-xs text-tx-2">{item.ownerId ? nameOf(item.ownerId) : '—'}</span>
                <span className="w-[160px] shrink-0 text-xs text-tx-1">
                  {t('postmortems.open_actions', { count: item.openActions })}
                  {item.overdueActions > 0 && (
                    <span className="ml-1.5 text-red">{t('postmortems.overdue', { count: item.overdueActions })}</span>
                  )}
                </span>
                <span className="w-[140px] shrink-0 text-xs tabular-nums text-tx-3">{formatMicrosActive(item.updatedAt, false)}</span>
              </li>
            ))}
          </ul>
        )}
      </AlertsPage>
      {opened && (
        <PostmortemDrawer open incidentId={opened.incidentId} incidentSummary={opened.incidentSummary} onClose={() => setOpened(null)} />
      )}
      <ResponseSettingsDrawer open={settingsOpen} onClose={() => setSettingsOpen(false)} />
    </>
  );
}
