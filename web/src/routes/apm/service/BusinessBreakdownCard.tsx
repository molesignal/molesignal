import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { Briefcase } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { businessContextApi, businessContextQueryKeys } from '@/api/businessContext';
import { tracesRedBreakdown } from '@/api/traceRed';
import type { TraceFilter } from '@/api/web';
import { cn } from '@/shell/lib/cn';
import { FittedSelect } from '@/shell/ui/fitted-select';

import { tracesHref } from '../endpointFilters';
import { formatCount, formatDuration } from '../format';
import { ApmCard } from '../frame/Page';

/** The most values listed. */
const SHOWN_VALUES = 6;
const NANOS_PER_MICRO = 1_000;

/**
 * The service's requests split by a business key (a tenant, a plan): how many
 * each value had, how many failed and how long the slowest twentieth took,
 * read from the Spans as the Traces page splits them. Any other field is a
 * click away there.
 */
export function BusinessBreakdownCard({
  orgId,
  service,
  range,
}: {
  orgId: string;
  service: string;
  /** Microseconds. */
  range: { from: number; to: number };
}) {
  const { t } = useTranslation('apm');
  const context = useQuery({
    queryKey: businessContextQueryKeys.context(orgId),
    queryFn: businessContextApi.get,
    enabled: Boolean(orgId),
  });
  const keys = (context.data?.keys ?? []).filter((key) => key.span);
  const [chosen, setChosen] = React.useState<string | null>(null);
  const key = keys.find((candidate) => candidate.key === chosen) ?? keys[0];
  const field = key?.span ?? null;
  const filters: TraceFilter[] = [{ field: 'service.name', op: '=', value: service }];
  const to = Math.max(range.to, range.from + NANOS_PER_MICRO);
  const breakdown = useQuery({
    queryKey: ['apm', 'business-breakdown', orgId, service, field, range.from, to],
    queryFn: ({ signal }) =>
      tracesRedBreakdown(
        { from: range.from, to, bucketTime: to - range.from, originTime: range.from, scope: 'entry', filters, by: field ?? '' },
        signal,
      ),
    enabled: Boolean(orgId) && Boolean(field),
    placeholderData: keepPreviousData,
    retry: false,
  });

  // Nothing declared, or nothing the Spans carry: the card has nothing to split by.
  if (!context.data || keys.length === 0) return null;

  const groups = (breakdown.data?.field === field ? breakdown.data.groups : []).slice(0, SHOWN_VALUES);
  const total = groups.reduce((sum, group) => sum + group.requests, 0);
  const important = new Set(key?.important ?? []);
  const href = `${tracesHref(filters, { from: range.from, to })}&tab=red${field ? `&by=${encodeURIComponent(field)}` : ''}`;

  let body: React.ReactNode;

  if (breakdown.isError && !breakdown.data) {
    body = <Note>{t('business.failed')}</Note>;
  } else if (!breakdown.data) {
    body = <Note busy>{t('business.loading')}</Note>;
  } else if (breakdown.data.unavailable) {
    body = <Note>{t('business.unavailable')}</Note>;
  } else if (groups.length === 0) {
    body = <Note>{t('business.empty')}</Note>;
  } else {
    body = (
      <div role="table" aria-label={key?.label} className={cn(breakdown.isFetching && 'opacity-60')}>
        <div role="row" className="grid h-8 grid-cols-[minmax(0,1fr)_72px_64px_72px] items-center gap-3 border-b border-bd-0 px-3 font-sans text-xs font-strong text-tx-2">
          <span role="columnheader">{key?.label}</span>
          <span role="columnheader" className="text-right">{t('business.requests')}</span>
          <span role="columnheader" className="text-right">{t('business.errors')}</span>
          <span role="columnheader" className="text-right">P95</span>
        </div>
        {groups.map((group) => {
          const failed = group.errors !== null && group.requests > 0 ? group.errors / group.requests : null;

          return (
            <div
              role="row"
              key={group.value ?? ''}
              data-apm-business-value={group.value ?? ''}
              className="grid h-8 grid-cols-[minmax(0,1fr)_72px_64px_72px] items-center gap-3 border-b border-bd-0 px-3 font-sans text-xs last:border-b-0"
            >
              <span role="cell" className="flex min-w-0 items-center gap-1.5">
                <span className={cn('truncate', group.value === null ? 'text-tx-3' : 'text-tx-0')}>
                  {group.value ?? t('business.no_value')}
                </span>
                {group.value !== null && important.has(group.value) && (
                  <span className="shrink-0 rounded bg-[var(--yellow-tint)] px-1 text-[var(--yellow-ink)]">{t('business.important')}</span>
                )}
                <span className="shrink-0 text-tx-3">{total > 0 ? `${Math.round((group.requests / total) * 100)}%` : ''}</span>
              </span>
              <span role="cell" className="text-right tabular-nums text-tx-1">{formatCount(group.requests)}</span>
              <span
                role="cell"
                className={cn('text-right tabular-nums', failed && failed >= 0.05 ? 'text-[var(--red-ink)]' : 'text-tx-2')}
              >
                {failed === null ? '—' : `${(failed * 100).toFixed(failed >= 0.1 ? 1 : 2)}%`}
              </span>
              <span role="cell" className="text-right tabular-nums text-tx-1">
                {group.p95Ns === null ? '—' : formatDuration(group.p95Ns / NANOS_PER_MICRO)}
              </span>
            </div>
          );
        })}
      </div>
    );
  }

  return (
    <ApmCard
      icon={Briefcase}
      title={t('business.title')}
      testId="business-breakdown"
      actions={
        <>
          {keys.length > 1 && (
            <FittedSelect
              aria-label={t('business.key')}
              value={key?.key ?? ''}
              onValueChange={setChosen}
              options={keys.map((option) => ({ value: option.key, label: option.label }))}
              maxWidth={200}
            />
          )}
          <Link to={href} className="font-sans text-xs font-strong text-tx-2 hover:text-tx-0 focus-visible:text-tx-0 focus-visible:outline-none">
            {t('business.any_field')}
          </Link>
        </>
      }
    >
      {body}
    </ApmCard>
  );
}

function Note({ children, busy = false }: { children: React.ReactNode; busy?: boolean }) {
  return (
    <p aria-busy={busy || undefined} className={cn('px-3 py-6 text-center font-sans text-xs text-tx-3', busy && 'animate-pulse motion-reduce:animate-none')}>
      {children}
    </p>
  );
}
