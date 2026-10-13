import { useQuery } from '@tanstack/react-query';
import { Archive, ArchiveRestore, ExternalLink, LockKeyhole, Trash2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as statusPagesApi from '@/api/statusPages';
import type { StatusPage } from '@/api/statusPages';
import { formatMicrosActive } from '@/lib/time';
import type { ActionAccess } from '@/product/actionAccess';
import { IconButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { SpectrumBar } from '@/shell/readout/SpectrumBar';

import { componentStatusLabel } from '../model';
import { publicStatusPageUrl } from '../publicUrl';
import {
  STATUS_COLOR,
  componentSpectrum,
  eventsUnderWay,
  maintenanceAhead,
  seenComponents,
} from '../summary';

/**
 * One status page in the list, as the answer its readers get from it now: how
 * the service stands, what is under way, and how its components divide.
 *
 * The list does not carry that state; each row reads its own page. The read is
 * the one the page's workspace opens with, so going in costs nothing more.
 */
export function StatusPageRow({
  page,
  manageAccess,
  restoring,
  onArchive,
  onRestore,
  onDelete,
}: {
  page: StatusPage;
  manageAccess: ActionAccess;
  restoring: boolean;
  onArchive: () => void;
  onRestore: () => void;
  onDelete: () => void;
}) {
  const { t } = useTranslation('status-pages');
  const archived = page.lifecycle === 'archived';
  const address = publicStatusPageUrl(page);

  return (
    <li
      data-status-page-row={page.id}
      className="relative grid min-h-[72px] grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-x-[14px] gap-y-[10px] px-[16px] py-[14px] transition-colors hover:bg-bg-2 lg:grid-cols-[auto_minmax(0,1.1fr)_minmax(0,1fr)_minmax(0,220px)_auto]"
    >
      <PageMark page={page} dimmed={archived} />

      <div className="min-w-0">
        <div className="flex min-w-0 items-center gap-[8px]">
          {/* The name is the way in; it is stretched over the row, and the row's own actions sit above it. */}
          <Link
            to={`/status-pages/${page.id}/overview`}
            className="truncate font-sans text-md font-strong text-tx-0 outline-none after:absolute after:inset-0 after:content-['']"
          >
            {page.name}
          </Link>
          {page.visibility === 'private' && (
            <span className="inline-flex shrink-0 items-center gap-[4px] text-xs text-tx-2">
              <LockKeyhole aria-hidden="true" className="h-3 w-3" />
              {t('visibility.private')}
            </span>
          )}
        </div>
        <div className="mt-[2px] truncate font-mono text-xs text-tx-2">{address}</div>
      </div>

      {archived ? (
        <p className="col-span-3 col-start-2 row-start-2 min-w-0 text-xs text-tx-2 lg:col-span-2 lg:col-start-3 lg:row-start-1">
          {t('list.archived_on', { date: page.archived_at ? formatMicrosActive(page.archived_at, false) : '—' })}
          {page.purge_after && (
            <span className="text-tx-3">
              {' · '}
              {t('list.removed_after', { date: formatMicrosActive(page.purge_after, false) })}
            </span>
          )}
        </p>
      ) : (
        <LiveState pageId={page.id} />
      )}

      <div className="relative z-[1] col-start-3 row-start-1 flex items-center gap-[2px] lg:col-start-5">
        {archived ? (
          <>
            <IconButton
              aria-label={t('actions.restore')}
              title={t('actions.restore')}
              disabled={manageAccess.disabled || restoring}
              disabledReason={manageAccess.reason}
              onClick={onRestore}
            >
              <ArchiveRestore className="h-3.5 w-3.5" />
            </IconButton>
            <IconButton
              aria-label={t('actions.delete')}
              title={t('actions.delete')}
              disabled={manageAccess.disabled}
              disabledReason={manageAccess.reason}
              className="enabled:hover:text-red-soft"
              onClick={onDelete}
            >
              <Trash2 className="h-3.5 w-3.5" />
            </IconButton>
          </>
        ) : (
          <>
            <IconButton
              aria-label={t('actions.open_public')}
              title={t('actions.open_public')}
              onClick={() => window.open(address, '_blank', 'noopener,noreferrer')}
            >
              <ExternalLink className="h-3.5 w-3.5" />
            </IconButton>
            <IconButton
              aria-label={t('actions.archive')}
              title={t('actions.archive')}
              disabled={manageAccess.disabled}
              disabledReason={manageAccess.reason}
              onClick={onArchive}
            >
              <Archive className="h-3.5 w-3.5" />
            </IconButton>
          </>
        )}
      </div>
    </li>
  );
}

/** Whose page it is: its logo, or its initial on its own colour. */
function PageMark({ page, dimmed }: { page: StatusPage; dimmed: boolean }) {
  if (page.logo_url) {
    return (
      <img
        src={page.logo_url}
        alt=""
        className={cn('h-[40px] w-[40px] shrink-0 rounded-lg bg-white object-contain p-[4px]', dimmed && 'opacity-50 grayscale')}
      />
    );
  }

  return (
    <span
      aria-hidden="true"
      className={cn(
        'grid h-[40px] w-[40px] shrink-0 place-items-center rounded-lg font-sans text-md font-bold text-white',
        dimmed && 'opacity-50 grayscale',
      )}
      style={{ backgroundColor: page.brand_color }}
    >
      {page.name.slice(0, 1).toUpperCase()}
    </span>
  );
}

function LiveState({ pageId }: { pageId: string }) {
  const { t } = useTranslation('status-pages');
  const query = useQuery({
    queryKey: ['status-pages', pageId, 'snapshot'],
    queryFn: () => statusPagesApi.get(pageId),
    staleTime: 30_000,
  });
  const cell = 'col-span-3 col-start-2 min-w-0 lg:col-span-1';

  if (query.isLoading) {
    return (
      <div
        aria-hidden="true"
        className={cn(cell, 'row-start-2 h-[32px] max-w-[260px] rounded-md bg-bg-2 lg:col-span-2 lg:col-start-3 lg:row-start-1')}
      />
    );
  }
  if (!query.data) {
    return (
      <p className={cn(cell, 'row-start-2 text-xs text-tx-3 lg:col-span-2 lg:col-start-3 lg:row-start-1')}>
        {t('list.state_unavailable')}
      </p>
    );
  }

  const snapshot = query.data;
  const components = seenComponents(snapshot);
  const underWay = eventsUnderWay(snapshot).length;
  const ahead = maintenanceAhead(snapshot).length;
  const goingOn = [
    underWay > 0 && t('list.under_way', { count: underWay }),
    ahead > 0 && t('list.scheduled', { count: ahead }),
  ].filter(Boolean);

  return (
    <>
      <div data-status-page-verdict={snapshot.overall_status} className={cn(cell, 'row-start-2 lg:col-start-3 lg:row-start-1')}>
        <div className="flex min-w-0 items-center gap-[8px] font-sans text-sm font-strong text-tx-0">
          <span
            aria-hidden="true"
            className="h-[8px] w-[8px] shrink-0 rounded-full"
            style={{ background: STATUS_COLOR[snapshot.overall_status] }}
          />
          <span className="truncate">{componentStatusLabel(t, snapshot.overall_status)}</span>
        </div>
        <p className="mt-[2px] truncate pl-[16px] text-xs text-tx-2">
          {goingOn.length > 0 ? goingOn.join(' · ') : t('list.nothing_under_way')}
        </p>
      </div>

      <div className={cn(cell, 'row-start-3 lg:col-start-4 lg:row-start-1')}>
        {components.length === 0 ? (
          <p className="text-xs text-tx-3">{t('list.no_components')}</p>
        ) : (
          <>
            <SpectrumBar
              label={t('sections.components')}
              segments={componentSpectrum(t, components)}
              legend={false}
            />
            <p className="mt-[6px] text-xs tabular-nums text-tx-2">
              {t('list.components', { count: components.length })}
            </p>
          </>
        )}
      </div>
    </>
  );
}
