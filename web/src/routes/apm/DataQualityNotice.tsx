import {
  AlertTriangle,
  ChevronDown,
  Clock3,
  ExternalLink,
  Layers3,
  X,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { ApmMeta } from '@/api/apm';
import { cn } from '@/shell/lib/cn';

import { formatCount, formatTimestamp } from './format';

export function DataQualityNotice({ meta }: { meta: ApmMeta }) {
  const { t } = useTranslation('apm');
  const [expanded, setExpanded] = React.useState(false);
  const [dismissed, setDismissed] = React.useState(false);
  const titleId = React.useId();
  const staleAfterMicros =
    meta.resolution === 'minute' ? 2 * 60 * 1_000_000 : 2 * 60 * 60 * 1_000_000;
  const stale = Boolean(
    meta.last_complete_bucket_at &&
      Date.now() * 1_000 - meta.last_complete_bucket_at > staleAfterMicros,
  );
  if (dismissed) return null;
  if (
    !meta.data_quality.partial &&
    meta.data_quality.overflow_dimensions.length === 0 &&
    !meta.activation_boundary &&
    !stale
  ) {
    return null;
  }
  const activationOnly =
    meta.activation_boundary && meta.data_quality.gaps.length === 0 && !stale;
  const partial = meta.data_quality.partial;
  const Icon = activationOnly || stale ? Clock3 : AlertTriangle;
  const hasDetails =
    meta.activation_boundary ||
    stale ||
    meta.data_quality.gaps.length > 0 ||
    meta.data_quality.overflow_dimensions.length > 0;
  return (
    <aside
      role="status"
      aria-labelledby={titleId}
      className={cn(
        'flex items-start gap-3 rounded-md border px-3.5 py-3 text-sm',
        activationOnly
          ? 'border-blue/30 bg-blue/5 text-tx-1'
          : 'border-yellow/30 bg-yellow/5 text-tx-1',
      )}
      data-testid="apm-data-quality"
    >
      <Icon
        aria-hidden
        className={cn(
          'mt-0.5 h-4 w-4 shrink-0',
          activationOnly ? 'text-blue' : 'text-yellow',
        )}
      />
      <div className="min-w-0 flex-1">
        <div id={titleId} className="font-strong text-tx-0">
          {activationOnly
            ? t('quality.activation_title')
            : partial
              ? t('quality.partial_title')
              : t('quality.stale_title')}
        </div>
        <div className="mt-0.5 text-xs leading-relaxed text-tx-2">
          {activationOnly
            ? t('quality.activation_description', {
                time: formatTimestamp(meta.projection_started_at),
              })
            : partial
              ? t('quality.partial_description', {
                  count: meta.data_quality.gaps.length,
                })
              : t('quality.stale_description', {
                  time: formatTimestamp(meta.last_complete_bucket_at),
                })}
        </div>
        {meta.data_quality.overflow_dimensions.length > 0 && (
          <div className="mt-2 flex flex-wrap items-center gap-1.5 text-xs text-tx-2">
            <Layers3 aria-hidden className="h-3.5 w-3.5" />
            <span>{t('quality.overflow')}:</span>
            {meta.data_quality.overflow_dimensions.map((dimension) => (
              <span
                key={dimension}
                className="rounded bg-bg-3 px-1.5 py-0.5 font-mono text-xs text-tx-1"
              >
                {dimension}
              </span>
            ))}
          </div>
        )}
        <div className="mt-3 flex flex-wrap items-center gap-2">
          {hasDetails && (
            <button
              type="button"
              aria-expanded={expanded}
              onClick={() => setExpanded((value) => !value)}
              className="inline-flex min-h-11 items-center gap-1.5 rounded px-2.5 text-sm font-strong text-tx-1 outline-none hover:bg-bg-2 focus-visible:bg-bg-2 sm:min-h-8 sm:text-xs"
            >
              {t('quality.view_details')}
              <ChevronDown
                aria-hidden
                className={cn(
                  'h-3.5 w-3.5 transition-transform',
                  expanded && 'rotate-180',
                )}
              />
            </button>
          )}
          <Link
            to="/datasource/applications/opentelemetry?signal=traces"
            className="inline-flex min-h-11 items-center gap-1.5 rounded px-2.5 text-sm font-strong text-indigo-soft outline-none hover:bg-bg-2 focus-visible:bg-bg-2 sm:min-h-8 sm:text-xs"
          >
            {t('quality.check_collection')}
            <ExternalLink aria-hidden className="h-3.5 w-3.5" />
          </Link>
          <button
            type="button"
            onClick={() => setDismissed(true)}
            className="inline-flex min-h-11 items-center gap-1.5 rounded px-2.5 text-sm text-tx-2 outline-none hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0 sm:min-h-8 sm:text-xs"
          >
            <X aria-hidden className="h-3.5 w-3.5" />
            {t('quality.dismiss')}
          </button>
        </div>
        {expanded && (
          <div
            className="mt-3 rounded-md bg-bg-2 px-3 py-2.5 text-xs text-tx-2"
            data-testid="apm-data-quality-details"
          >
            <p>{t('quality.impact_description')}</p>
            <ul className="mt-2 space-y-2">
              {meta.activation_boundary && (
                <li>
                  <span className="font-strong text-tx-1">
                    {t('quality.unavailable_range')}:
                  </span>{' '}
                  {formatTimestamp(meta.range.from)} –{' '}
                  {formatTimestamp(meta.projection_started_at)}
                </li>
              )}
              {stale && (
                <li>
                  <span className="font-strong text-tx-1">
                    {t('quality.delayed_range')}:
                  </span>{' '}
                  {formatTimestamp(meta.last_complete_bucket_at)} –{' '}
                  {formatTimestamp(meta.range.to)}
                </li>
              )}
              {meta.data_quality.gaps.map((gap) => (
                <li key={`${gap.recorded_at}:${gap.reason}`}>
                  <span className="font-strong text-tx-1">
                    {t(`quality.reasons.${gap.reason}`)}
                  </span>{' '}
                  · {formatTimestamp(gap.range.start)} – {formatTimestamp(gap.range.end)}
                  {' · '}
                  {t('quality.dropped_facts', {
                    count: formatCount(gap.dropped_facts),
                  })}
                </li>
              ))}
            </ul>
          </div>
        )}
      </div>
    </aside>
  );
}
