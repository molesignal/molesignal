import { X } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { FieldTypeChip, type FieldTypeTone } from '@/shell/query/fields/TypeChip';
import { FieldValueList } from '@/shell/query/FieldValues';
import { Skeleton } from '@/shell/ui/skeleton';

import { FieldValueBarsChart } from './Bars';
import {
  formatShape,
  formatShare,
  formatStatistic,
  formatWrittenNumber,
  hasCommonValues,
  type FieldAnalysis,
  type FieldAnalysisKind,
} from './model';
import type { FieldAnalysisState } from './source';
import { FieldValueTrendChart, trendColor } from './Trend';

/** What the rows that were read are: the counts are named with it. */
export type FieldAnalysisSubject = 'logs' | 'spans' | 'traces';

export interface FieldAnalysisPanelProps {
  /** The field, as its row in the list shows it. */
  name: string;
  chip: { glyph: string; tone: FieldTypeTone };
  /**
   * How its values are read: a count of nanoseconds since the epoch is a
   * moment, whatever it is stored as.
   */
  kind: FieldAnalysisKind;
  subject: FieldAnalysisSubject;
  state: FieldAnalysisState;
  /** What was read, in the reader's words: everything the query matches, or the loaded rows. */
  scope: string;
  /** How a value of the field is written, where it has a way of its own: a duration, a moment. */
  describe?: ((value: string) => string) | undefined;
  onClose: () => void;
  className?: string | undefined;
}

/** A block of the panel: a small card on the panel's ground, as a field is in the list beside it. */
const BLOCK = 'rounded-md bg-[var(--functional-surface)] p-3 shadow-sm';
const HEADING = 'mb-2 font-sans text-xs font-strong text-tx-1';

function Figure({ label, value, hint }: { label: string; value: React.ReactNode; hint?: string | undefined }) {
  return (
    <div className="flex min-w-0 items-baseline justify-between gap-2" title={hint}>
      <dt className="min-w-0 truncate font-sans text-xs text-tx-2">{label}</dt>
      <dd className="min-w-0 truncate text-right font-mono text-xs font-body tabular-nums text-tx-0">{value}</dd>
    </div>
  );
}

/**
 * What a field holds across the results of a query, beside the field list it
 * was asked from: how many rows have a value, how many different values there
 * are, the commonest of them, and for a number what it runs from and to and
 * how it is spread.
 */
export function FieldAnalysisPanel({
  name,
  chip,
  kind,
  subject,
  state,
  scope,
  describe,
  onClose,
  className,
}: FieldAnalysisPanelProps) {
  const { t } = useTranslation('common');
  const panel = React.useRef<HTMLElement>(null);
  const { analysis, pending, unavailable } = state;
  const fallback = state.scope === 'loaded' && unavailable !== null && unavailable !== 'field_protected';

  // The panel takes the focus when it opens, so it is read out and Escape closes it, and gives
  // it back to the button it was opened from.
  React.useEffect(() => {
    const opener = document.activeElement;
    const element = panel.current;

    element?.focus({ preventScroll: true });

    return () => {
      const held = element?.contains(document.activeElement) || document.activeElement === document.body;
      if (!held || !(opener instanceof HTMLElement) || !opener.isConnected) return;

      opener.focus({ preventScroll: true });
      // The button shows only while the pointer or the focus is on its row. With neither, the
      // row itself takes the keyboard, and the button comes into view with it.
      if (document.activeElement !== opener) {
        opener.closest('[data-field-row]')?.querySelector<HTMLElement>('button')?.focus({ preventScroll: true });
      }
    };
  }, []);

  return (
    <section
      ref={panel}
      tabIndex={-1}
      aria-label={t('field_analysis.title_aria', { name })}
      data-field-analysis={name}
      data-field-analysis-scope={pending ? 'pending' : state.scope}
      onKeyDown={(event) => {
        if (event.key !== 'Escape') return;

        event.stopPropagation();
        onClose();
      }}
      className={cn(
        // It floats over the results: as tall as what it holds, lifted off what it covers.
        'flex min-h-0 flex-col overflow-hidden rounded-md bg-[var(--functional-surface)] shadow-drawer focus-visible:bg-[var(--functional-surface)]',
        className,
      )}
    >
      <header className="flex shrink-0 items-center gap-2 pl-3 pr-1 pt-1">
        <FieldTypeChip glyph={chip.glyph} tone={chip.tone} />
        <h2 className="min-w-0 flex-1 truncate font-sans text-sm font-strong text-tx-0" title={name}>
          {name}
        </h2>
        <button
          type="button"
          onClick={onClose}
          aria-label={t('field_analysis.close')}
          title={t('field_analysis.close')}
          className="grid h-8 w-8 shrink-0 place-items-center rounded-md text-tx-3 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0"
        >
          <X className="h-3.5 w-3.5" aria-hidden="true" />
        </button>
      </header>
      {/* What the numbers below are of, said before them: a count means nothing without it. */}
      <p data-field-analysis-scope-line className="shrink-0 px-3 pb-2 font-sans text-xs text-tx-2">
        {scope}
        {fallback && (
          <>
            {' · '}
            {t(`field_analysis.fallback.${unavailable}`)}{' '}
            <button
              type="button"
              onClick={state.retry}
              className="rounded font-strong text-[var(--blue-ink)] hover:underline focus-visible:underline"
            >
              {t('actions.retry')}
            </button>
          </>
        )}
      </p>
      <div className="mx-2 mb-2 flex min-h-0 flex-col gap-1.5 overflow-auto rounded-md bg-[var(--page-canvas)] p-1.5">
        {pending ? (
          <div role="status" aria-label={t('field_analysis.loading', { name })} className={cn(BLOCK, 'flex flex-col gap-2')}>
            {['w-2/3', 'w-1/2', 'w-3/4', 'w-1/3'].map((width) => (
              <Skeleton key={width} className={`h-[14px] ${width}`} />
            ))}
          </div>
        ) : analysis ? (
          <Findings
            name={name}
            kind={kind}
            subject={subject}
            analysis={analysis}
            estimated={state.scope === 'all'}
            describe={describe}
          />
        ) : (
          <p role={unavailable === 'field_protected' ? undefined : 'alert'} className={cn(BLOCK, 'font-sans text-xs text-tx-2')}>
            {unavailable === 'field_protected' ? (
              t('field_analysis.protected')
            ) : (
              <>
                {t('field_analysis.unavailable')}{' '}
                <button
                  type="button"
                  onClick={state.retry}
                  className="rounded font-strong text-[var(--blue-ink)] hover:underline focus-visible:underline"
                >
                  {t('actions.retry')}
                </button>
              </>
            )}
          </p>
        )}
      </div>
    </section>
  );
}

interface FindingsProps {
  name: string;
  kind: FieldAnalysisKind;
  subject: FieldAnalysisSubject;
  analysis: FieldAnalysis;
  /** The quartiles were read in one pass over everything, from a digest: they are estimates. */
  estimated: boolean;
  describe: ((value: string) => string) | undefined;
}

function Findings({ name, kind, subject, analysis, estimated, describe }: FindingsProps) {
  const { t } = useTranslation('common');
  const { total, present, distinct, range, spread, bars } = analysis;
  const missing = Math.max(0, total - present);
  // A value is written the way the field has of its own, or as a number where it is one. Text
  // stays as it is: an id made of digits is not a number to group the digits of.
  const written = (value: string) =>
    describe ? describe(value) : kind === 'number' ? formatWrittenNumber(value) : value;
  const measured = (value: number | null) =>
    value === null ? '—' : describe ? describe(String(value)) : formatStatistic(value);
  // A shape has no unit: it is a number whatever the values are.
  const shape = (value: number | null) => (value === null ? '—' : formatShape(value));
  const common = hasCommonValues(analysis);
  // The values followed over time are the first of the commonest, in their order.
  const trend = common && kind !== 'time' ? analysis.trend : null;

  return (
    <>
      <section className={BLOCK} data-field-analysis-block="basics">
        <h3 className={HEADING}>{t('field_analysis.basics')}</h3>
        <dl className="grid grid-cols-2 gap-x-5 gap-y-2">
          <Figure label={t(`field_analysis.subjects.${subject}.total`)} value={total.toLocaleString()} />
          <Figure label={t(`field_analysis.subjects.${subject}.present`)} value={present.toLocaleString()} />
          <Figure
            label={t('field_analysis.distinct')}
            value={`${analysis.distinctExact ? '' : '≈ '}${distinct.toLocaleString()}`}
            hint={analysis.distinctExact ? undefined : t('field_analysis.estimated')}
          />
          <Figure
            label={t(`field_analysis.subjects.${subject}.missing`)}
            value={`${missing.toLocaleString()}${missing > 0 ? ` (${formatShare(missing, total)})` : ''}`}
          />
        </dl>
      </section>
      {present === 0 && (
        <p className={cn(BLOCK, 'font-sans text-xs text-tx-2')} data-field-analysis-block="none">
          {t(`field_analysis.subjects.${subject}.none`)}
        </p>
      )}
      {kind === 'time' && range && (
        <section className={BLOCK} data-field-analysis-block="range">
          <h3 className={HEADING}>{t('field_analysis.statistics')}</h3>
          <dl className="grid grid-cols-1 gap-y-2">
            <Figure label={t('field_analysis.earliest')} value={written(range.min)} />
            <Figure label={t('field_analysis.latest')} value={written(range.max)} />
          </dl>
        </section>
      )}
      {kind === 'number' && range && spread && (
        <section className={BLOCK} data-field-analysis-block="statistics">
          <h3 className={HEADING}>{t('field_analysis.statistics')}</h3>
          <dl className="grid grid-cols-2 gap-x-5 gap-y-2">
            <Figure label={t('field_analysis.max')} value={written(range.max)} />
            <Figure label={t('field_analysis.min')} value={written(range.min)} />
            <Figure label={t('field_analysis.avg')} value={measured(spread.avg)} />
            <Figure label={t('field_analysis.median')} value={measured(spread.median)} />
            <Figure label={t('field_analysis.q1')} value={measured(spread.q1)} />
            <Figure label={t('field_analysis.q3')} value={measured(spread.q3)} />
            <Figure label={t('field_analysis.stddev_samp')} value={measured(spread.stddevSamp)} />
            <Figure label={t('field_analysis.stddev_pop')} value={measured(spread.stddevPop)} />
            <Figure
              label={t('field_analysis.kurtosis')}
              value={shape(spread.kurtosis)}
              hint={t('field_analysis.kurtosis_hint')}
            />
            <Figure
              label={t('field_analysis.skewness')}
              value={shape(spread.skewness)}
              hint={t('field_analysis.skewness_hint')}
            />
          </dl>
          {estimated && (
            <p className="type-micro mt-2 font-sans text-tx-3">{t('field_analysis.quartiles_estimated')}</p>
          )}
        </section>
      )}
      {kind === 'number' && bars && (
        <section className={BLOCK} data-field-analysis-block="distribution">
          <h3 className={HEADING}>{t('field_analysis.distribution')}</h3>
          <FieldValueBarsChart bars={bars} name={name} format={(value) => measured(value)} />
        </section>
      )}
      {trend && (
        <section className={BLOCK} data-field-analysis-block="trend">
          <h3 className={HEADING}>{t('field_analysis.trend', { count: trend.series.length })}</h3>
          <FieldValueTrendChart
            trend={trend}
            label={written}
            ariaLabel={t('field_analysis.trend_aria', { name, count: trend.series.length })}
          />
        </section>
      )}
      {present > 0 && kind !== 'time' && (
        <section className={BLOCK} data-field-analysis-block="top">
          <h3 className={HEADING}>{t('field_analysis.top')}</h3>
          {common ? (
            <FieldValueList
              className="-mx-[6px]"
              items={analysis.top.map((item, index) => ({
                key: item.value,
                label: written(item.value),
                title: item.value,
                count: item.count,
                countLabel: item.count.toLocaleString(),
                countTitle: formatShare(item.count, present),
                share: item.count / present,
                // The list is the legend of the chart above it: a value has the colour of its line.
                ...(trend && trend.series[index]?.value === item.value ? { swatch: trendColor(index) } : {}),
                actions: [],
              }))}
            />
          ) : (
            <p className="font-sans text-xs text-tx-2">{t('field_analysis.all_different')}</p>
          )}
        </section>
      )}
    </>
  );
}
