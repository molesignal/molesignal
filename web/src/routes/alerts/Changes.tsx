import { useQuery } from '@tanstack/react-query';
import { ArrowUpRight, Search } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { DataTable } from '@/admin';
import { type ChangeEntry, type ChangeKind, changesApi, changesQueryKeys } from '@/api/changes';
import { formatMicrosActive } from '@/lib/time';
import type { ProductStateProps } from '@/product/states';
import { ChangeRail } from '@/shell/changes/ChangeMarks';
import { CHANGE_ICON, changeHref, changeTone } from '@/shell/changes/model';
import { ChromeButton } from '@/shell/chrome';
import { FormInput, FormSelect } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { useAuthStore } from '@/stores/auth';

import { AlertsPage } from './Frame';
import { alertFlatTableClassName } from './Surfaces';

const HOUR_MICROS = 3_600_000_000;
const SPANS = { '24h': 24, '7d': 7 * 24, '30d': 30 * 24 } as const;
type Span = keyof typeof SPANS;
const KINDS: readonly ChangeKind[] = ['deployment', 'configuration', 'feature_flag', 'scaling', 'infrastructure', 'annotation', 'version', 'rule'];

/** What a change was of, in a line: a service and its version, or a title. */
function subjectOf(change: ChangeEntry): string {
  if (change.kind === 'annotation' || change.kind === 'rule') return change.title ?? '';
  return [change.service, change.version].filter(Boolean).join(' ');
}

/**
 * Every change of the organization over a stretch of time, the latest first:
 * what pipelines and tools reported, annotations people put on a time range,
 * versions APM first saw in spans, and alert rule edits, each kind as far as
 * the reader may read it. The rail above the list marks them across the
 * stretch.
 */
export function AlertsChanges() {
  const { t } = useTranslation('alerts');
  const { t: tc } = useTranslation('common');
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const [span, setSpan] = React.useState<Span>('7d');
  const [kind, setKind] = React.useState<ChangeKind | ''>('');
  const [search, setSearch] = React.useState('');
  // The stretch is settled when it is chosen, not at every render.
  const [to, setTo] = React.useState(() => Date.now() * 1_000);
  const params = { from: to - SPANS[span] * HOUR_MICROS, to, ...(kind ? { kinds: [kind] } : {}) };

  const query = useQuery({
    queryKey: changesQueryKeys.list(orgId, params),
    queryFn: () => changesApi.list(params),
    enabled: Boolean(orgId),
  });

  const rows = React.useMemo(() => {
    const needle = search.trim().toLocaleLowerCase();
    const all = query.data ?? [];
    if (!needle) return all;

    return all.filter((change) =>
      [change.service, change.version, change.title, change.description, change.author, change.commit]
        .filter(Boolean)
        .join(' ')
        .toLocaleLowerCase()
        .includes(needle),
    );
  }, [query.data, search]);

  const state: ProductStateProps | null = query.isLoading
    ? { variant: 'loading' }
    : query.isError
      ? {
          variant: 'error',
          title: t('changes.error_title'),
          error: query.error,
          action: <ChromeButton onClick={() => void query.refetch()}>{t('center.retry')}</ChromeButton>,
        }
      : rows.length === 0
        ? { variant: 'empty', title: t('changes.empty_title'), description: t('changes.empty_description') }
        : null;

  return (
    <AlertsPage
      state={state}
      listHead={
        <>
          <label className="relative w-full min-w-0 sm:w-[280px]">
            <span className="sr-only">{t('changes.search_label')}</span>
            <Search className="pointer-events-none absolute left-3 top-2.5 h-4 w-4 text-tx-3" />
            <FormInput type="search" value={search} onChange={(event) => setSearch(event.target.value)} placeholder={t('changes.search_placeholder')} className="pl-9" />
          </label>
          <FormSelect
            ariaLabel={t('changes.kind_label')}
            className="w-full sm:w-[180px]"
            value={kind}
            onChange={(value) => setKind(value as ChangeKind | '')}
            options={[{ value: '', label: t('changes.all_kinds') }, ...KINDS.map((item) => ({ value: item, label: tc(`changes.kind.${item}`) }))]}
          />
          <FormSelect
            ariaLabel={t('changes.span_label')}
            className="w-full sm:w-[140px]"
            value={span}
            onChange={(value) => {
              setSpan(value as Span);
              setTo(Date.now() * 1_000);
            }}
            options={(Object.keys(SPANS) as Span[]).map((item) => ({ value: item, label: t(`changes.span.${item}`) }))}
          />
        </>
      }
    >
      <ChangeRail changes={rows} start={params.from} end={params.to} className="mx-3 my-2" />
      <DataTable
        className={alertFlatTableClassName}
        rows={rows}
        rowKey={(change) => change.id}
        columns={[
          {
            key: 'at',
            header: t('changes.columns.at'),
            width: 180,
            cell: (change) => <span className="tabular-nums text-tx-2">{formatMicrosActive(change.at)}</span>,
          },
          {
            key: 'kind',
            header: t('changes.columns.kind'),
            width: 160,
            cell: (change) => {
              const Icon = CHANGE_ICON[change.kind];
              return (
                <span data-change-kind={change.kind} className={cn('inline-flex items-center gap-1.5', changeTone(change))}>
                  <Icon aria-hidden className="h-3.5 w-3.5" />
                  {tc(`changes.kind.${change.kind}`)}
                </span>
              );
            },
          },
          {
            key: 'subject',
            header: t('changes.columns.subject'),
            cell: (change) => (
              <span className="flex min-w-0 flex-col">
                <span className="truncate font-strong text-tx-0">{subjectOf(change) || '—'}</span>
                {change.description && change.kind !== 'deployment' && <span className="truncate text-tx-3">{change.description}</span>}
              </span>
            ),
          },
          {
            key: 'status',
            header: t('changes.columns.status'),
            width: 110,
            cell: (change) => (change.status ? <span className={changeTone(change)}>{tc(`changes.status.${change.status}`)}</span> : <span className="text-tx-4">—</span>),
          },
          {
            key: 'by',
            header: t('changes.columns.by'),
            width: 180,
            cell: (change) => <span className="truncate text-tx-2">{[change.author, change.commit?.slice(0, 7)].filter(Boolean).join(' · ') || '—'}</span>,
          },
          {
            key: 'open',
            header: <span className="sr-only">{t('changes.columns.open')}</span>,
            width: 48,
            cell: (change) => {
              const href = changeHref(change);
              return href ? (
                <Link to={href} aria-label={t('changes.open', { change: subjectOf(change) || tc(`changes.kind.${change.kind}`) })} className="text-tx-3 hover:text-tx-0 focus-visible:text-tx-0 focus-visible:outline-none">
                  <ArrowUpRight aria-hidden className="h-3.5 w-3.5" />
                </Link>
              ) : null;
            },
          },
        ]}
      />
    </AlertsPage>
  );
}
