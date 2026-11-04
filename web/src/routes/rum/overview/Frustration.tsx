import { MousePointerClick } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { RumAggregate } from '@/api/rum';
import { cn } from '@/shell/lib/cn';

import { formatCount } from './format';
import {
  clause,
  explorerPath,
  type FrustrationKind,
  pageField,
  readFrustration,
  type OverviewScope,
} from './model';
import { CardState, OverviewCard } from './OverviewCard';

/** The ink each sign is told in, here and wherever the tabs name it. */
export const KIND_TEXT: Record<FrustrationKind, string> = {
  rage: 'text-[var(--orange-ink)]',
  dead: 'text-[var(--yellow-ink)]',
  error: 'text-[var(--red-ink)]',
};

const ROW = 'relative grid min-h-10 items-center gap-3 border-b border-[var(--row-rule)] px-3 py-1 font-sans hover:bg-bg-2 focus-within:bg-bg-2';
const TRACKS = 'minmax(0,1fr) minmax(88px,auto) minmax(40px,auto)';

/**
 * The elements users were frustrated by most: the ones they clicked again and again, clicked and
 * got nothing from, or clicked into an error. Each is named with the page it is on, what most of
 * the frustrated actions on it were, and how many there were. A row opens the explorer on those actions.
 */
export function FrustrationCard({
  query,
  scope,
  browser,
  language,
}: {
  query: { isPending: boolean; isError: boolean; error: unknown; refetch: () => unknown; data: RumAggregate | undefined };
  scope: OverviewScope;
  browser: boolean;
  language: string;
}) {
  const { t } = useTranslation('rum');
  const rows = readFrustration(query.data);
  const pages = pageField(browser ? 'browser' : 'flutter');

  return (
    <OverviewCard
      icon={MousePointerClick}
      tone="text-[var(--orange-ink)]"
      title={t('overview.frustration.title')}
      description={t(browser ? 'overview.frustration.description_click' : 'overview.frustration.description_tap')}
      testId="frustration"
    >
      <CardState query={query} empty={rows.length === 0} emptyTitle={t('overview.frustration.empty')}>
        <ul data-rum-frustration="" className="m-0 list-none p-0">
          {rows.map((row) => {
            const clauses = [
              'action.frustration.count >= 1',
              ...(row.element ? [clause('action.target.name', row.element)] : []),
              ...(row.page ? [clause(pages, row.page)] : []),
            ];
            const name = row.element ?? t('overview.frustration.unnamed');

            return (
              <li
                key={`${row.element ?? ''}|${row.page ?? ''}`}
                data-rum-frustrated={row.element ?? ''}
                className={ROW}
                style={{ gridTemplateColumns: TRACKS }}
              >
                <span className="min-w-0">
                  <Link
                    to={explorerPath('actions', scope, clauses)}
                    className={cn(
                      'block truncate text-sm outline-none after:absolute after:inset-0 after:content-[""]',
                      row.element ? 'font-strong text-tx-0' : 'text-tx-3',
                    )}
                  >
                    {name}
                  </Link>
                  {row.page && <span className="block truncate text-xs text-tx-3">{row.page}</span>}
                </span>
                <span className={cn('text-sm font-strong', KIND_TEXT[row.kind])}>
                  {t(`overview.frustration.kinds.${browser ? row.kind : `${row.kind}_tap`}`)}
                </span>
                <span className="text-right text-sm tabular-nums text-tx-1">{formatCount(row.total, language)}</span>
              </li>
            );
          })}
        </ul>
      </CardState>
    </OverviewCard>
  );
}
