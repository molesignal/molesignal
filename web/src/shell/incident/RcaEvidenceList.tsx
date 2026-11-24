import type { TFunction } from 'i18next';
import { ArrowUpRight } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { useTimeFormatter } from '@/lib/time';
import { cn } from '@/shell/lib/cn';
import type { RcaEvidence } from '@/types/alerting';

import { citedLabels, evidenceHref, insightFigure } from './rcaEvidence';

/** The figures of a piece, written out; `null` when it has none or they were left out. */
function figuresOf(piece: RcaEvidence, t: TFunction, locale: string): string | null {
  const figures = piece.figures ?? {};
  const number = (value: number | undefined) => (value ?? 0).toLocaleString(locale, { maximumFractionDigits: 0 });
  const ms = (micros: number | undefined) => `${((micros ?? 0) / 1_000).toLocaleString(locale, { maximumFractionDigits: 1 })} ms`;

  switch (piece.kind) {
    case 'insight': {
      if (figures.value === undefined) return null;
      const value = (figure: number) => insightFigure(piece.detail, figure, locale);
      const parts = [t('drawer.rca.evidence.figures.now', { value: value(figures.value) })];
      if (figures.previous !== undefined) parts.push(t('drawer.rca.evidence.figures.previous', { value: value(figures.previous) }));
      if (figures.lastWeek !== undefined) parts.push(t('drawer.rca.evidence.figures.last_week', { value: value(figures.lastWeek) }));
      return parts.join(t('drawer.rca.evidence.separator'));
    }
    case 'new_error_group':
      return figures.occurrences === undefined ? null : t('drawer.rca.evidence.figures.occurrences', { count: figures.occurrences, formatted: number(figures.occurrences) });
    case 'sample_trace':
      return figures.duration === undefined ? null : ms(figures.duration);
    case 'error_logs':
      return figures.now === undefined ? null : t('drawer.rca.evidence.figures.error_logs', { now: number(figures.now), lastWeek: number(figures.lastWeek) });
    case 'new_log_pattern':
      return figures.lines === undefined ? null : t('drawer.rca.evidence.figures.lines', { count: Math.round(figures.lines), formatted: number(figures.lines) });
    case 'business_impact':
      return figures.errors === undefined
        ? null
        : t('drawer.rca.evidence.figures.business', { errors: number(figures.errors), requests: number(figures.requests) });
    case 'attribute_skew': {
      if (figures.failedShare === undefined) return null;
      const percent = (share: number | undefined) => (share ?? 0).toLocaleString(locale, { style: 'percent', maximumFractionDigits: 0 });
      return t('drawer.rca.evidence.figures.skew', { failed: percent(figures.failedShare), others: percent(figures.otherShare) });
    }
    default:
      return null;
  }
}

/**
 * The evidence a root-cause analysis rests on, in the order labelled: what
 * each piece is, the service, the figures, and a link to where it can be
 * looked at. Pieces the summary cites carry their label in full ink. Figures
 * read from data the reader may not read are left out and say so.
 */
export function RcaEvidenceList({ evidence, summary }: { evidence: readonly RcaEvidence[]; summary: string }) {
  const { t, i18n } = useTranslation('alerts');
  const time = useTimeFormatter();
  const cited = new Set(citedLabels(summary));
  const locale = i18n.resolvedLanguage ?? i18n.language;

  if (evidence.length === 0) return null;

  return (
    <div data-rca-evidence="" className="flex flex-col gap-1">
      <span className="font-sans text-xs font-strong text-tx-1">{t('drawer.rca.evidence.title')}</span>
      <ul className="divide-y divide-bd-0 rounded-md bg-bg-2">
        {evidence.map((piece) => {
          const href = evidenceHref(piece);
          const figures = figuresOf(piece, t, locale);
          const detail =
            piece.kind === 'insight' && piece.detail
              ? t(`drawer.rca.evidence.insight.${piece.detail.split('.')[0]}`, { defaultValue: piece.detail })
              : (piece.kind === 'business_impact' || piece.kind === 'attribute_skew') && piece.detail
                ? piece.detail
                : null;

          return (
            <li key={piece.label} data-rca-evidence-label={piece.label} data-kind={piece.kind} className="flex min-h-7 flex-wrap items-baseline gap-x-2 gap-y-0.5 px-3 py-1 font-sans text-xs">
              <span className={cn('w-7 shrink-0 tabular-nums', cited.has(piece.label) ? 'font-strong text-tx-0' : 'text-tx-3')}>{piece.label}</span>
              <span className="shrink-0 text-tx-2">{t(`drawer.rca.evidence.kind.${piece.kind}`)}</span>
              {piece.service && <span className="shrink-0 font-strong text-tx-0">{piece.service}</span>}
              {detail && <span className="shrink-0 text-tx-1">{detail}</span>}
              {piece.subject && (
                <span className="min-w-0 flex-1 truncate text-tx-1" title={piece.subject}>
                  {piece.subject}
                </span>
              )}
              {figures && <span className="shrink-0 tabular-nums text-tx-0">{figures}</span>}
              {piece.withheld && <span className="shrink-0 text-tx-3">{t('drawer.rca.evidence.withheld')}</span>}
              {piece.at !== undefined && <span className="shrink-0 tabular-nums text-tx-3">{time.micros(piece.at, false)}</span>}
              {href && (
                <Link to={href} aria-label={t('drawer.rca.evidence.open', { label: piece.label })} className="ml-auto shrink-0 text-tx-3 hover:text-tx-0 focus-visible:text-tx-0 focus-visible:outline-none">
                  <ArrowUpRight aria-hidden className="h-3 w-3" />
                </Link>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
