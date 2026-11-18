import { useQuery } from '@tanstack/react-query';
import { ArrowUpRight, Plus, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as dashboardsApi from '@/api/dashboards';
import * as streamsApi from '@/api/streams';
import { cn } from '@/shell/lib/cn';
import { Input } from '@/shell/ui/input';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/shell/ui/select';

import {
  dashboardHref,
  isLinkAddress,
  isSelector,
  logSourceHref,
  MAX_FILTER_LENGTH,
  MAX_LINK_NAME_LENGTH,
  MAX_LINK_URL_LENGTH,
  MAX_SELECTOR_LENGTH,
  MAX_SOURCES_PER_KIND,
  metricSourceHref,
  type SourceKind,
  type SourceProblem,
  type SourceRows,
} from './dataSources';

/** Where a row's data opens: a page of the console, or an address elsewhere. */
type Target = { to: string; external: boolean } | null;

const ROW_FIELD = 'h-8 min-w-0 text-xs';

/**
 * Where a service's data is, as the catalog keeps it: the log streams that
 * hold its logs (with the condition that picks them out), the selectors of
 * its metrics, the dashboards that show it, and pages about it elsewhere.
 * Each row opens its data once it says enough to.
 */
export function DataSourcesEditor({
  rows,
  onChange,
  problem,
  disabled,
}: {
  rows: SourceRows;
  onChange: (rows: SourceRows) => void;
  problem: SourceProblem | null;
  disabled: boolean;
}) {
  const { t } = useTranslation('apm');
  const streamList = React.useId();
  // Offered, not required: a reader who may not list them still types stream names and keeps the dashboards kept.
  const streams = useQuery({ queryKey: ['streams', 'list'], queryFn: () => streamsApi.list(200), staleTime: 60_000, retry: false });
  const dashboards = useQuery({ queryKey: ['dashboards', 'list'], queryFn: () => dashboardsApi.list(), staleTime: 60_000, retry: false });
  const logStreams = (streams.data ?? []).filter((stream) => stream.streamType === 'logs').map((stream) => stream.name);

  // A dashboard the list does not hold (gone, or not readable) still shows, by its id.
  const dashboardOptions = React.useMemo(() => {
    const known = (dashboards.data ?? []).map((dashboard) => ({ id: dashboard.id, title: dashboard.title }));
    const ids = new Set(known.map((dashboard) => dashboard.id));
    const kept = rows.dashboards.filter((id) => id && !ids.has(id)).map((id) => ({ id, title: id }));
    return [...known, ...kept];
  }, [dashboards.data, rows.dashboards]);

  const set = <K extends SourceKind>(kind: K, next: SourceRows[K]) => onChange({ ...rows, [kind]: next });

  return (
    <div data-apm-definition-sources="" className="space-y-3">
      <div className="space-y-0.5">
        <span className="block font-strong text-tx-1">{t('definition.sources.title')}</span>
        <span className="block text-tx-3">{t('definition.sources.hint')}</span>
      </div>
      <datalist id={streamList}>
        {logStreams.map((name) => (
          <option key={name} value={name} />
        ))}
      </datalist>
      <Group
        kind="logs"
        label={t('definition.sources.logs')}
        rows={rows.logs}
        blank={() => ({ stream: '', filter: '' })}
        onChange={(next) => set('logs', next)}
        disabled={disabled}
        problem={problem}
        target={(row) => (row.stream.trim() ? { to: logSourceHref({ stream: row.stream.trim(), filter: row.filter.trim() || null }), external: false } : null)}
        render={(row, change) => (
          <>
            <Input
              aria-label={t('definition.sources.stream')}
              placeholder={t('definition.sources.stream')}
              list={streamList}
              value={row.stream}
              disabled={disabled}
              maxLength={255}
              onChange={(event) => change({ ...row, stream: event.target.value })}
              className={cn(ROW_FIELD, 'w-[38%] shrink-0')}
              data-apm-source-stream=""
            />
            <Input
              aria-label={t('definition.sources.filter')}
              title={t('definition.sources.filter')}
              placeholder="service_name = 'checkout'"
              value={row.filter}
              disabled={disabled}
              maxLength={MAX_FILTER_LENGTH}
              onChange={(event) => change({ ...row, filter: event.target.value })}
              className={cn(ROW_FIELD, 'flex-1 font-mono')}
              data-apm-source-filter=""
            />
          </>
        )}
      />
      <Group
        kind="metrics"
        label={t('definition.sources.metrics')}
        rows={rows.metrics}
        blank={() => ''}
        onChange={(next) => set('metrics', next)}
        disabled={disabled}
        problem={problem}
        target={(row) => (isSelector(row) ? { to: metricSourceHref({ selector: row.trim() }), external: false } : null)}
        render={(row, change) => (
          <Input
            aria-label={t('definition.sources.selector')}
            placeholder='{service="checkout"}'
            value={row}
            disabled={disabled}
            maxLength={MAX_SELECTOR_LENGTH}
            onChange={(event) => change(event.target.value)}
            className={cn(ROW_FIELD, 'flex-1 font-mono')}
            data-apm-source-selector=""
          />
        )}
      />
      <Group
        kind="dashboards"
        label={t('definition.sources.dashboards')}
        rows={rows.dashboards}
        blank={() => ''}
        onChange={(next) => set('dashboards', next)}
        disabled={disabled}
        problem={problem}
        target={(row) => (row.trim() ? { to: dashboardHref(row.trim()), external: false } : null)}
        render={(row, change) => (
          <Select value={row} onValueChange={(next) => next && change(next)} disabled={disabled}>
            <SelectTrigger aria-label={t('definition.sources.dashboard')} className="h-8 min-w-0 flex-1 text-xs" data-apm-source-dashboard="">
              <SelectValue placeholder={t('definition.sources.choose_dashboard')} />
            </SelectTrigger>
            <SelectContent>
              {dashboardOptions.map((dashboard) => (
                <SelectItem key={dashboard.id} value={dashboard.id}>
                  {dashboard.title}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        )}
      />
      <Group
        kind="links"
        label={t('definition.sources.links')}
        rows={rows.links}
        blank={() => ({ name: '', url: '' })}
        onChange={(next) => set('links', next)}
        disabled={disabled}
        problem={problem}
        target={(row) => (isLinkAddress(row.url) ? { to: row.url.trim(), external: true } : null)}
        render={(row, change) => (
          <>
            <Input
              aria-label={t('definition.sources.link_name')}
              placeholder={t('definition.sources.link_name')}
              value={row.name}
              disabled={disabled}
              maxLength={MAX_LINK_NAME_LENGTH}
              onChange={(event) => change({ ...row, name: event.target.value })}
              className={cn(ROW_FIELD, 'w-[34%] shrink-0')}
              data-apm-source-link-name=""
            />
            <Input
              aria-label={t('definition.sources.link_url')}
              inputMode="url"
              placeholder="https://wiki.example.com/checkout"
              value={row.url}
              disabled={disabled}
              maxLength={MAX_LINK_URL_LENGTH}
              onChange={(event) => change({ ...row, url: event.target.value })}
              className={cn(ROW_FIELD, 'flex-1')}
              data-apm-source-link-url=""
            />
          </>
        )}
      />
      {problem && (
        <span className="block text-[var(--red-ink)]" data-apm-source-problem={problem.problem}>
          {t(`definition.sources.problem.${problem.problem}`)}
        </span>
      )}
    </div>
  );
}

/** One kind of data source: its rows, each with an open and a remove control, and an add control. */
function Group<T>({
  kind,
  label,
  rows,
  blank,
  onChange,
  disabled,
  problem,
  target,
  render,
}: {
  kind: SourceKind;
  label: string;
  rows: readonly T[];
  blank: () => T;
  onChange: (rows: T[]) => void;
  disabled: boolean;
  problem: SourceProblem | null;
  target: (row: T) => Target;
  render: (row: T, change: (row: T) => void) => React.ReactNode;
}) {
  const { t } = useTranslation('apm');

  return (
    <div data-apm-source-group={kind} className="space-y-1.5">
      <div className="flex items-center justify-between gap-2">
        <span className="text-tx-2">{label}</span>
        <button
          type="button"
          disabled={disabled || rows.length >= MAX_SOURCES_PER_KIND}
          onClick={() => onChange([...rows, blank()])}
          data-apm-source-add={kind}
          className="inline-flex h-6 items-center gap-1 rounded-md px-1.5 font-strong text-tx-2 outline-none hover:bg-bg-2 hover:text-tx-1 focus-visible:bg-bg-2 disabled:opacity-40"
        >
          <Plus aria-hidden className="h-3 w-3" />
          {t('definition.sources.add')}
        </button>
      </div>
      {rows.map((row, index) => {
        const opens = target(row);
        const wrong = problem?.kind === kind && problem.index === index;

        return (
          <div key={index} data-apm-source-row={`${kind}-${index}`} className="flex items-center gap-1.5">
            <div className={cn('flex min-w-0 flex-1 items-center gap-1.5', wrong && '[&_input]:text-[var(--red-ink)]')} aria-invalid={wrong || undefined}>
              {render(row, (next) => onChange(rows.map((item, at) => (at === index ? next : item))))}
            </div>
            <OpenControl target={opens} label={t('definition.sources.open')} />
            <button
              type="button"
              aria-label={t('definition.sources.remove')}
              title={t('definition.sources.remove')}
              disabled={disabled}
              onClick={() => onChange(rows.filter((_, at) => at !== index))}
              data-apm-source-remove=""
              className="inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-tx-3 outline-none hover:bg-bg-2 hover:text-tx-1 focus-visible:bg-bg-2 disabled:opacity-40"
            >
              <Trash2 aria-hidden className="h-3.5 w-3.5" />
            </button>
          </div>
        );
      })}
    </div>
  );
}

/** Opens a row's data, in the console or in a new tab for an address elsewhere; room kept while the row says too little. */
function OpenControl({ target, label }: { target: Target; label: string }) {
  const className =
    'inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-tx-3 outline-none hover:bg-bg-2 hover:text-tx-1 focus-visible:bg-bg-2';

  if (!target) return <span aria-hidden className="w-7 shrink-0" />;

  if (target.external) {
    return (
      <a href={target.to} target="_blank" rel="noreferrer" aria-label={label} title={label} className={className} data-apm-source-open="">
        <ArrowUpRight aria-hidden className="h-3.5 w-3.5" />
      </a>
    );
  }

  return (
    <Link to={target.to} aria-label={label} title={label} className={className} data-apm-source-open="">
      <ArrowUpRight aria-hidden className="h-3.5 w-3.5" />
    </Link>
  );
}
