import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { type ErrorRow } from './model';
import { HEAD, ListScroller, LONG_LIST, Moment, NoRows, rowClass } from './parts';
import { useKeepInSight, useLitRow, useSeekable } from './useFollow';
import { formatClock } from '../../replay/clock';
import type { ReplayController } from '../../replay/controller';

const TRACKS = '64px minmax(96px,168px) minmax(0,1fr) 132px';

/** The page that tells an issue of an application: its errors, with the issue open. */
const issuePath = (application: string, issue: string): string => `/rum/errors?${new URLSearchParams({ app: application, issue })}`;

/**
 * What went wrong in the session, in the order it happened: when, what kind of error, what it said and
 * the issue it belongs to. The issue is the same one the errors page tracks, so a row leads to what is
 * known of it. The row the replay is at is lit, and a row moves the replay to its moment.
 */
export function ErrorList({
  rows,
  application,
  controller,
  zero,
  empty,
  notice,
}: {
  rows: readonly ErrorRow[];
  application: string;
  controller: ReplayController;
  zero: number;
  empty: string;
  notice?: string | undefined;
}) {
  const { t } = useTranslation('rum');
  const list = React.useRef<HTMLDivElement>(null);
  const lit = useLitRow(controller, rows);
  const seekable = useSeekable(controller);
  const long = rows.length > LONG_LIST;

  useKeepInSight(list, lit);

  if (rows.length === 0) return <NoRows>{empty}</NoRows>;

  return (
    <>
      <ListScroller ref={list} label={t('session_detail.tools.errors.aria')} minWidth={560}>
        <div role="row" data-list-head="" className={HEAD} style={{ gridTemplateColumns: TRACKS }}>
          <span role="columnheader">{t('session_detail.tools.errors.time')}</span>
          <span role="columnheader">{t('session_detail.tools.errors.type')}</span>
          <span role="columnheader">{t('session_detail.tools.errors.message')}</span>
          <span role="columnheader">{t('session_detail.tools.errors.issue')}</span>
        </div>
        {rows.map((row, index) => (
          <div
            key={row.key}
            role="row"
            data-rum-error={row.key}
            data-current={lit === index ? 'true' : undefined}
            className={rowClass(lit === index, long)}
            style={{ gridTemplateColumns: TRACKS }}
          >
            <Moment
              at={row.at}
              zero={zero}
              seekable={seekable}
              label={t('session_detail.tools.seek', { time: formatClock((row.at - zero) / 1_000, true) })}
              onSeek={() => controller.seekToMicros(row.at)}
            />
            <span role="cell" className="min-w-0 truncate font-strong text-[var(--red-ink)]" title={row.type ?? undefined}>
              {row.crash ? t('session_detail.tools.errors.crash') : (row.type ?? '—')}
            </span>
            <span role="cell" className="min-w-0 truncate text-tx-0" title={row.message}>
              {row.message}
            </span>
            <span role="cell" className="min-w-0">
              {row.issue ? (
                <Link
                  to={issuePath(application, row.issue)}
                  aria-label={t('session_detail.tools.errors.open_issue', { id: row.issue })}
                  data-rum-issue-link={row.issue}
                  className="relative z-[1] block truncate rounded-sm font-mono text-[var(--blue-ink)] underline-offset-4 outline-none hover:underline focus-visible:underline"
                >
                  {t('session_detail.tools.errors.issue_id', { id: row.issue.slice(0, 8) })}
                </Link>
              ) : (
                <span className="text-tx-4">—</span>
              )}
            </span>
          </div>
        ))}
      </ListScroller>
      {notice && (
        <p data-rum-tool-notice="" className="m-0 border-t border-[var(--row-rule)] px-3 py-2 font-sans text-xs text-tx-3">
          {notice}
        </p>
      )}
    </>
  );
}
