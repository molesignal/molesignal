import { useQuery } from '@tanstack/react-query';
import { Search } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useSearchParams } from 'react-router-dom';

import { DataTable } from '@/admin';
import { ENTITY_KINDS, type EntityKind, type EntityRow, entitiesApi, entityQueryKeys } from '@/api/entities';
import type { ProductStateProps } from '@/product/states';
import { ListPage } from '@/product/templates';
import { ChromeButton, Dot } from '@/shell/chrome';
import { FormInput } from '@/shell/FormDrawer';
import { FittedSelect } from '@/shell/ui/fitted-select';
import { useAuthStore } from '@/stores/auth';
import { formatRelativeMicros } from '@/time/relative';

import { entityHref, isCurrent } from './model';

const ALL = 'all';

/** Where an entity sits, from its attributes: a pod's namespace and node, a node's cluster. */
function whereOf(row: EntityRow): string {
  const { attributes } = row;
  if (row.kind === 'pod') return [attributes['k8s.cluster.name'], attributes['k8s.namespace.name'], attributes['k8s.node.name']].filter(Boolean).join(' / ');
  if (row.kind === 'node') return attributes['k8s.cluster.name'] ?? '';
  if (row.kind === 'container') return [attributes['k8s.namespace.name'], attributes['k8s.pod.name']].filter(Boolean).join(' / ');
  return [attributes['cloud.provider'], attributes['cloud.region']].filter(Boolean).join(' / ');
}

/**
 * Every cluster, node, host, pod and container seen over the last week, the
 * latest seen first, each with how many services run on it. The kind and a
 * search narrow it, and are kept in the address.
 */
export function Infrastructure() {
  const { t, i18n } = useTranslation('infrastructure');
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const [params, setParams] = useSearchParams();
  const kindParam = params.get('kind');
  const kind = ENTITY_KINDS.includes(kindParam as EntityKind) ? (kindParam as EntityKind) : undefined;
  const [search, setSearch] = React.useState(params.get('q') ?? '');
  const deferred = React.useDeferredValue(search.trim());
  const listParams = { ...(kind ? { kind } : {}), ...(deferred ? { search: deferred } : {}) };
  const now = Date.now() * 1_000;

  const query = useQuery({
    queryKey: entityQueryKeys.list(orgId, listParams),
    queryFn: () => entitiesApi.list(listParams),
    enabled: Boolean(orgId),
    staleTime: 30_000,
  });
  const rows = query.data ?? [];

  const narrow = (name: string, value: string) => {
    const next = new URLSearchParams(params);
    if (value) next.set(name, value);
    else next.delete(name);
    setParams(next, { replace: true });
  };

  const narrowed = Boolean(kind || deferred);
  const state: ProductStateProps | null = query.isLoading
    ? { variant: 'loading' }
    : query.isError
      ? {
          variant: 'error',
          title: t('list.error_title'),
          error: query.error,
          action: <ChromeButton onClick={() => void query.refetch()}>{t('list.retry')}</ChromeButton>,
        }
      : rows.length === 0
        ? narrowed
          ? { variant: 'empty', title: t('list.no_match_title'), description: t('list.no_match_description') }
          : { variant: 'empty', title: t('list.empty_title'), description: t('list.empty_description') }
        : null;

  return (
    <ListPage
      appearance="surface"
      title={t('title')}
      subtitle={t('subtitle')}
      state={state}
      listHead={
        <>
          <label className="relative w-full min-w-0 sm:w-[280px]">
            <span className="sr-only">{t('list.search_label')}</span>
            <Search className="pointer-events-none absolute left-3 top-2.5 h-4 w-4 text-tx-3" />
            <FormInput
              type="search"
              value={search}
              onChange={(event) => {
                setSearch(event.target.value);
                narrow('q', event.target.value.trim());
              }}
              placeholder={t('list.search_placeholder')}
              className="pl-9"
            />
          </label>
          <FittedSelect
            aria-label={t('list.kind_label')}
            value={kind ?? ALL}
            onValueChange={(value) => narrow('kind', value === ALL ? '' : value)}
            options={[{ value: ALL, label: t('kind.all') }, ...ENTITY_KINDS.map((item) => ({ value: item, label: t(`kind.${item}`) }))]}
            maxWidth={180}
          />
        </>
      }
    >
      <DataTable
        rows={rows}
        rowKey={(row) => `${row.kind}:${row.key}`}
        columns={[
          {
            key: 'name',
            header: t('list.columns.name'),
            cell: (row) => (
              <span className="flex min-w-0 items-center gap-2">
                <Dot tone={isCurrent(row, now) ? 'green' : 'dim'} />
                <Link
                  to={entityHref(row.kind, row.key)}
                  data-entity-row={`${row.kind}:${row.key}`}
                  aria-label={t('list.open', { name: row.name })}
                  title={row.key}
                  className="min-w-0 truncate font-strong text-tx-0 hover:underline focus-visible:underline focus-visible:outline-none"
                >
                  {row.name}
                </Link>
              </span>
            ),
          },
          { key: 'kind', header: t('list.columns.kind'), width: 110, cell: (row) => <span className="text-tx-2">{t(`kind.${row.kind}`)}</span> },
          { key: 'where', header: t('list.columns.where'), cell: (row) => <span className="truncate text-tx-2">{whereOf(row) || '—'}</span> },
          { key: 'services', header: t('list.columns.services'), width: 100, className: 'text-right', headerClassName: 'text-right', cell: (row) => <span className="tabular-nums text-tx-0">{row.services}</span> },
          {
            key: 'last_seen',
            header: t('list.columns.last_seen'),
            width: 140,
            className: 'text-right',
            headerClassName: 'text-right',
            cell: (row) => <span className="tabular-nums text-tx-2">{formatRelativeMicros(row.lastSeenAt, i18n.language)}</span>,
          },
        ]}
      />
    </ListPage>
  );
}
