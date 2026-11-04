import type { TFunction } from 'i18next';

import type { ServiceTier } from '@/api/apm';
import { cn } from '@/shell/lib/cn';
import { Sparkline } from '@/shell/readout/Sparkline';
import { formatRelativeMicros } from '@/time/relative';
import type { Incident } from '@/types/alerting';

import { formatDuration, formatRate, formatThroughput } from '../format';
import { type CatalogColumn, type CatalogRow, instanceCount, tagsOf, throughputOf } from './model';
import { SEVERITY_TEXT } from '../frame/alerts';
import { apdexTone } from '../overview/model';
import { ENDPOINT_KIND_TONE } from '../service/Overview';
import { entryKindsOf, languageColor, languageOf, stackOf } from '../service/serviceFacts';

/** Each tier as a word in its own ink: the two that matter most stand out. */
export const TIER_CLASS: Record<ServiceTier, string> = {
  p0: 'font-strong text-[var(--red-ink)]',
  p1: 'font-strong text-[var(--orange-ink)]',
  p2: 'font-strong text-tx-1',
  p3: 'text-tx-1',
  p4: 'text-tx-2',
  p5: 'text-tx-2',
};

/** Apdex as a word in the tone of its band. */
const APDEX_TEXT = {
  good: 'text-tx-0',
  warn: 'font-strong text-[var(--yellow-ink)]',
  danger: 'font-strong text-[var(--red-ink)]',
} as const;

/** The inks a tag's key is drawn in, the same key always in the same one. */
const TAG_INKS = ['text-[var(--blue-ink)]', 'text-[var(--green-ink)]', 'text-[var(--purple-ink)]', 'text-[var(--chart-5)]', 'text-[var(--chart-2)]'];

export function tagInk(key: string): string {
  let hash = 0;
  for (const char of key) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  return TAG_INKS[hash % TAG_INKS.length]!;
}

export interface CatalogNames {
  team: (id: string) => string;
  user: (id: string) => string;
  onCall: (teamId: string) => string | null;
  /** The incidents going on about a row's service. */
  alerts: (row: CatalogRow) => readonly Incident[];
}

const QUIET = <span className="text-tx-4">—</span>;

/** A service's tags as words: each key in its own ink, its value after it. */
export function TagWords({ tags, limit = 3 }: { tags: ReadonlyArray<[string, string]>; limit?: number }) {
  if (tags.length === 0) return QUIET;
  const shown = tags.slice(0, limit);

  return (
    <span className="truncate" title={tags.map(([key, value]) => (value ? `${key}: ${value}` : key)).join('\n')}>
      {shown.map(([key, value], index) => (
        <span key={key} data-apm-tag={key}>
          {index > 0 && <span className="text-tx-4"> · </span>}
          <span className={cn('font-strong', tagInk(key))}>{key}</span>
          {value && <span className="text-tx-0">: {value}</span>}
        </span>
      ))}
      {tags.length > limit && <span className="text-tx-3"> +{tags.length - limit}</span>}
    </span>
  );
}

export function CatalogCell({
  column,
  row,
  range,
  names,
  t,
  locale,
  absolute,
}: {
  column: CatalogColumn;
  row: CatalogRow;
  range: { from: Date; to: Date };
  names: CatalogNames;
  t: TFunction<'apm'>;
  locale: string;
  absolute: (micros: number | null | undefined, withSeconds?: boolean) => string;
}) {
  const { summary, definition } = row;
  const red = summary?.red;
  const spark = summary?.sparkline ?? [];

  switch (column) {
    case 'environment':
      return row.environment ? <span className="truncate text-tx-0">{row.environment}</span> : QUIET;
    case 'team':
      return definition?.teamId ? <span className="truncate text-tx-0">{names.team(definition.teamId)}</span> : QUIET;
    case 'owner':
      return definition?.ownerId ? <span className="truncate text-tx-0">{names.user(definition.ownerId)}</span> : QUIET;
    case 'on_call': {
      const user = definition?.teamId ? names.onCall(definition.teamId) : null;
      return user ? <span className="truncate text-tx-0">{names.user(user)}</span> : QUIET;
    }
    case 'tier':
      return definition?.tier ? <span className={TIER_CLASS[definition.tier]}>{t(`definition.tier.${definition.tier}`)}</span> : QUIET;
    case 'alerts': {
      const alerts = names.alerts(row);
      const worst = alerts[0]?.severity;
      return worst ? (
        <span data-apm-alert-count={alerts.length} className={cn('truncate font-strong', SEVERITY_TEXT[worst])} title={alerts.map((incident) => incident.summary).join('\n')}>
          {t('alerts.active', { count: alerts.length })}
        </span>
      ) : (
        QUIET
      );
    }
    case 'lifecycle':
      return definition?.lifecycle ? <span className="truncate text-tx-0">{t(`definition.lifecycle.${definition.lifecycle}`)}</span> : QUIET;
    case 'tags':
      return <TagWords tags={tagsOf(row)} />;
    case 'language': {
      const stack = stackOf(summary);
      return stack ? (
        <span className="flex min-w-0 items-center gap-1.5 text-tx-0">
          <span aria-hidden className="h-2 w-2 shrink-0 rounded-full" style={{ background: languageColor(languageOf(summary)) ?? 'var(--tx-3)' }} />
          <span className="truncate">{stack}</span>
        </span>
      ) : (
        QUIET
      );
    }
    case 'type': {
      const kinds = entryKindsOf(summary?.profile);
      return kinds.length ? (
        <span className="truncate">
          {kinds.map((kind, index) => (
            <span key={kind} className={ENDPOINT_KIND_TONE[kind]}>
              {index > 0 && <span className="font-body text-tx-3"> · </span>}
              {t(`service.facts.kinds.${kind}`)}
            </span>
          ))}
        </span>
      ) : (
        QUIET
      );
    }
    case 'region': {
      const regions = summary?.profile?.regions ?? [];
      return regions[0] ? (
        <span className="truncate text-tx-0" title={regions.map((value) => value.value).join(', ')}>
          {regions[0].value}
          {regions.length > 1 && <span className="text-tx-3"> +{regions.length - 1}</span>}
        </span>
      ) : (
        QUIET
      );
    }
    case 'throughput':
      if (!red) return QUIET;
      return (
        <>
          <Sparkline values={spark.map((point) => point.requestCount)} variant="bars" zeroBased className="h-4 w-14 text-[var(--chart-1)]" />
          <span className="w-[72px] shrink-0 tabular-nums text-tx-0">{formatThroughput(throughputOf(red.requestCount, range))}</span>
        </>
      );
    case 'error_rate':
      if (!red || red.requestCount === 0) return QUIET;
      return (
        <>
          <Sparkline
            values={spark.map((point) => (point.requestCount > 0 ? point.errorCount / point.requestCount : null))}
            zeroBased
            className={cn('h-4 w-14', red.errorCount > 0 ? 'text-red' : 'text-tx-4')}
          />
          <span className={cn('w-[52px] shrink-0 tabular-nums', red.errorCount > 0 ? 'font-strong text-[var(--red-ink)]' : 'text-tx-0')}>
            {formatRate(red.errorRate)}
          </span>
        </>
      );
    case 'p50':
    case 'p90':
    case 'p95':
    case 'p99': {
      const micros = red?.[`${column}Time`];
      return micros == null ? QUIET : <span className="tabular-nums text-tx-0">{formatDuration(micros)}</span>;
    }
    case 'average':
      return red?.durationAverage == null ? QUIET : <span className="tabular-nums text-tx-0">{formatDuration(red.durationAverage)}</span>;
    case 'apdex':
      return red?.apdex == null ? (
        QUIET
      ) : (
        <span
          className={cn('tabular-nums', APDEX_TEXT[apdexTone(red.apdex) ?? 'good'])}
          title={summary?.apdexThreshold ? t('apdex.threshold_title', { threshold: formatDuration(summary.apdexThreshold) }) : undefined}
        >
          {red.apdex.toFixed(2)}
        </span>
      );
    case 'instances':
      return summary ? <span className="tabular-nums text-tx-0">{instanceCount(summary)}</span> : QUIET;
    case 'version':
      return summary?.currentVersion ? (
        <span className="truncate text-tx-0" title={summary.currentVersion}>
          {summary.currentVersion}
        </span>
      ) : (
        QUIET
      );
    case 'last_deploy':
      return summary?.currentVersionSince ? (
        <span className="truncate text-tx-0" title={absolute(summary.currentVersionSince, false)}>
          {formatRelativeMicros(summary.currentVersionSince, locale)}
        </span>
      ) : (
        QUIET
      );
    case 'last_seen':
      return summary ? (
        <span className="truncate text-tx-0" title={absolute(summary.lastSeenAt, false)}>
          {formatRelativeMicros(summary.lastSeenAt, locale)}
        </span>
      ) : (
        QUIET
      );
  }
}
