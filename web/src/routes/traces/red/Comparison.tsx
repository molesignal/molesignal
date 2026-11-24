import { RefreshCw } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { TraceRedCompareResponse, TraceRedDimension, TraceRedFigures, TraceRedValueShare } from '@/api/traceRed';
import { cn } from '@/shell/lib/cn';
import { Button } from '@/shell/ui/button';
import { FittedSelect } from '@/shell/ui/fitted-select';

import { COMPARE_CHOICES, type CompareChoice, choiceParams, formatShare } from './compare';
import { formatDuration, formatRequests } from './format';

/** The most fields shown, and values of each. */
const SHOWN_FIELDS = 8;
const SHOWN_VALUES = 3;
const NONE = 'none';

interface RedComparisonProps {
  choice: CompareChoice | null;
  onChoiceChange: (choice: CompareChoice | null) => void;
  /** The figures of every request: which sets can be told, and the percentiles a slow set begins at. */
  total: TraceRedFigures;
  response: TraceRedCompareResponse | undefined;
  pending: boolean;
  error: unknown;
  onRetry: () => void;
  /** Break the requests down by this field. */
  onBreakDown: (field: string) => void;
  /** Narrow the query to the requests with this value of the field. */
  onDrill: (field: string, value: string) => void;
}

/**
 * How the failed or the slow requests differ from the others the query
 * matches, field by field: the fields whose values the two sets share least
 * first, each with the values whose shares differ most. Read from a sample of
 * each set, which it says.
 */
export function RedComparison({
  choice,
  onChoiceChange,
  total,
  response,
  pending,
  error,
  onRetry,
  onBreakDown,
  onDrill,
}: RedComparisonProps) {
  const { t, i18n } = useTranslation('traces');
  const locale = i18n.resolvedLanguage ?? i18n.language;

  const options = [
    { value: NONE, label: t('explore.red.compare.choice.none') },
    ...COMPARE_CHOICES.map((option) => ({
      value: option,
      label:
        option === 'failed'
          ? t('explore.red.compare.choice.failed')
          : t(`explore.red.compare.choice.${option}`, {
              duration: formatDuration(option === 'p95' ? total.p95Ns : total.p99Ns),
            }),
      disabled: choiceParams(option, total) === null,
    })),
  ];

  let body: React.ReactNode = null;

  if (choice !== null) {
    if (error && !response) {
      body = (
        <Note text={t('explore.red.compare.failed')}>
          <Button variant="outline" size="sm" onClick={onRetry}>
            <RefreshCw aria-hidden className="h-3.5 w-3.5" />
            {t('explore.red.states.retry')}
          </Button>
        </Note>
      );
    } else if (!response) {
      body = <Note text={t('explore.red.compare.loading')} busy={pending} />;
    } else if (response.unavailable) {
      body = <Note text={t(`explore.red.states.unavailable.${response.unavailable}`)} />;
    } else if (response.dimensions.length === 0) {
      body = <Note text={t(response.selected === 0 ? 'explore.red.compare.none_chosen' : 'explore.red.compare.alike')} />;
    } else {
      body = (
        <ol data-red-compare-list className={cn('divide-y divide-bd-0', pending && 'opacity-60')}>
          {response.dimensions.slice(0, SHOWN_FIELDS).map((dimension) => (
            <DimensionRow key={dimension.field} dimension={dimension} onBreakDown={onBreakDown} onDrill={onDrill} />
          ))}
        </ol>
      );
    }
  }

  return (
    <section data-red-compare className="min-w-0 rounded-md bg-[var(--functional-surface)] pb-2">
      <div className="flex min-h-11 flex-wrap items-center gap-x-2 gap-y-1 px-4 py-2 font-sans text-xs text-tx-1">
        <span className="font-medium">{t('explore.red.compare.title')}</span>
        <FittedSelect
          aria-label={t('explore.red.compare.choice_label')}
          value={choice ?? NONE}
          onValueChange={(value) => onChoiceChange(value === NONE ? null : (value as CompareChoice))}
          options={options}
          maxWidth={240}
        />
        {choice !== null && response && !response.unavailable && (
          <span data-red-compare-sample className="text-tx-3">
            {t(response.sampled ? 'explore.red.compare.sample_sampled' : 'explore.red.compare.sample', {
              chosen: formatRequests(response.selected, locale),
              others: formatRequests(response.baseline, locale),
            })}
          </span>
        )}
      </div>
      {body}
    </section>
  );
}

function DimensionRow({
  dimension,
  onBreakDown,
  onDrill,
}: {
  dimension: TraceRedDimension;
  onBreakDown: (field: string) => void;
  onDrill: (field: string, value: string) => void;
}) {
  const { t } = useTranslation('traces');

  return (
    <li data-red-compare-field={dimension.field} className="grid gap-2 px-4 py-2 font-sans text-xs [@container_(min-width:640px)]:grid-cols-[minmax(0,1fr)_minmax(0,2fr)]">
      <div className="min-w-0">
        <button
          type="button"
          onClick={() => onBreakDown(dimension.field)}
          title={t('explore.red.compare.break_down')}
          className="block max-w-full truncate rounded text-left font-strong text-tx-0 outline-none hover:text-indigo-soft focus-visible:bg-bg-3"
        >
          {dimension.field}
        </button>
        <span className="text-tx-3">{t('explore.red.compare.score', { score: formatShare(dimension.score) })}</span>
      </div>
      <ul className="flex min-w-0 flex-col gap-1.5">
        {dimension.values.slice(0, SHOWN_VALUES).map((share) => (
          <ValueRow key={share.value ?? ''} field={dimension.field} share={share} onDrill={onDrill} />
        ))}
      </ul>
    </li>
  );
}

function ValueRow({
  field,
  share,
  onDrill,
}: {
  field: string;
  share: TraceRedValueShare;
  onDrill: (field: string, value: string) => void;
}) {
  const { t } = useTranslation('traces');
  const value = share.value;

  return (
    <li data-red-compare-value={value ?? ''} className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-3 gap-y-0.5">
      {value === null ? (
        <span className="truncate text-tx-3">{t('explore.red.breakdown.no_value')}</span>
      ) : (
        <button
          type="button"
          onClick={() => onDrill(field, value)}
          title={t('explore.red.breakdown.drill', { clause: `${field} = ${value}` })}
          className="min-w-0 truncate rounded text-left text-tx-1 outline-none hover:text-indigo-soft focus-visible:bg-bg-3"
        >
          {value}
        </button>
      )}
      <span className="tabular-nums text-tx-2">
        {t('explore.red.compare.shares', { chosen: formatShare(share.selected), others: formatShare(share.baseline) })}
      </span>
      <Bars share={share} />
    </li>
  );
}

/** The value's share of the chosen requests over its share of the others, on tracks as long as the row. */
function Bars({ share }: { share: TraceRedValueShare }) {
  const width = (value: number) => `${Math.max(value * 100, value > 0 ? 1 : 0)}%`;

  return (
    <span aria-hidden="true" className="col-span-2 flex flex-col gap-[2px]">
      <span className="block h-[3px] w-full rounded-full bg-[var(--control-surface)]">
        <span data-red-compare-chosen className="block h-full rounded-full bg-orange" style={{ width: width(share.selected) }} />
      </span>
      <span className="block h-[3px] w-full rounded-full bg-[var(--control-surface)]">
        <span className="block h-full rounded-full bg-tx-3" style={{ width: width(share.baseline) }} />
      </span>
    </span>
  );
}

function Note({ text, busy = false, children }: { text: string; busy?: boolean; children?: React.ReactNode }) {
  return (
    <div
      data-red-compare-note
      aria-busy={busy || undefined}
      className="flex min-h-20 flex-col items-center justify-center gap-3 px-4 py-5 text-center font-sans text-xs text-tx-2"
    >
      <span className={cn(busy && 'animate-pulse motion-reduce:animate-none')}>{text}</span>
      {children}
    </div>
  );
}
