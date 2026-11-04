import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { Flag } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useSearchParams } from 'react-router-dom';

import * as rumApi from '@/api/rum';
import { widenTimeWindow } from '@/investigation/timeRangeRecovery';
import { ProductState } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { useAuthStore } from '@/stores/auth';

import { comparisonQuery, flagField, readVariants, type Variant } from './model';
import { formatDurationNs, formatShare } from '../_helpers';
import { RUM_WEEK_WINDOW } from '../explorer/timeWindow';
import { formatCount } from '../overview/format';
import { clause, explorerPath, isBrowser, scopeFilters } from '../overview/model';
import { CardState, OverviewCard } from '../overview/OverviewCard';
import { useAggregate } from '../overview/useOverview';
import { BODY_ROW, HEAD_ROW } from '../performance/tableParts';
import { type ScopeContext, ScopedFrame } from '../ScopedFrame';

const TRACKS = 'minmax(120px,1.4fr) minmax(64px,0.5fr) repeat(2,minmax(136px,0.7fr))';
const BROWSER_TRACKS = 'minmax(120px,1.4fr) minmax(64px,0.5fr) repeat(2,minmax(136px,0.7fr)) repeat(3,minmax(64px,0.5fr)) 96px';

/** The tab of the feature flags: which flags sessions met, and how the sessions of each value did. */
export function RumFeatureFlags() {
  return (
    <ScopedFrame name="flags" opensOn={RUM_WEEK_WINDOW}>
      {(context) => <FlagsView context={context} />}
    </ScopedFrame>
  );
}

function FlagsView({ context }: { context: ScopeContext }) {
  const { t } = useTranslation('rum');
  const { scope, range, language, window, setWindow } = context;
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const [params, setParams] = useSearchParams();

  const keys = useQuery({
    queryKey: ['rum', 'flags', orgId, scope, range],
    queryFn: ({ signal }) =>
      rumApi.keys({ type: 'views', from: range.from, to: range.to, filters: scopeFilters(scope), column: 'feature_flags', limit: 200 }, signal),
    enabled: Boolean(orgId),
    placeholderData: keepPreviousData,
    staleTime: 30_000,
  });

  const flags = keys.data?.keys ?? [];
  const asked = params.get('flag');
  const open = asked ?? flags[0]?.key ?? null;

  const choose = (flag: string) =>
    setParams(
      (current) => {
        const next = new URLSearchParams(current);

        next.set('flag', flag);

        return next;
      },
      { replace: true },
    );

  if (keys.isSuccess && flags.length === 0 && !keys.data?.unavailable) {
    return (
      <ProductState
        variant="empty"
        title={t('flags.empty.title')}
        description={t('flags.empty.description')}
        action={<ChromeButton onClick={() => setWindow(widenTimeWindow(window))}>{t('overview.empty.widen')}</ChromeButton>}
      />
    );
  }

  return (
    <div className="grid min-w-0 items-start gap-[12px] xl:grid-cols-[320px_minmax(0,1fr)]">
      <OverviewCard icon={Flag} title={t('flags.list_title')} description={t('flags.list_description')} testId="flags-list" className="min-w-0">
        <CardState query={keys} empty={flags.length === 0} emptyTitle={t('flags.empty.title')}>
          <ul aria-label={t('flags.list_title')} className="m-0 max-h-[calc(100vh-260px)] list-none overflow-auto p-0">
            {flags.map((flag) => (
              <li key={flag.key}>
                <button
                  type="button"
                  data-rum-flag={flag.key}
                  aria-current={open === flag.key ? 'true' : undefined}
                  onClick={() => choose(flag.key)}
                  className={cn(
                    'flex w-full items-baseline justify-between gap-3 border-0 border-b border-[var(--row-rule)] bg-transparent px-3 py-2 text-left font-sans text-xs outline-none hover:bg-bg-2 focus-visible:bg-bg-2',
                    open === flag.key && 'bg-[var(--floating-item-selected)]',
                  )}
                >
                  <span className="min-w-0 truncate font-mono text-tx-0" title={flag.key}>
                    {flag.key}
                  </span>
                  <span className="shrink-0 tabular-nums text-tx-3">{t('flags.sessions', { count: flag.sessions, value: formatCount(flag.sessions, language) })}</span>
                </button>
              </li>
            ))}
          </ul>
        </CardState>
      </OverviewCard>
      {open && <Comparison key={open} flag={open} context={context} />}
    </div>
  );
}

/** How the sessions of each value of a flag did, side by side: what is worst of a column stands out. */
function Comparison({ flag, context }: { flag: string; context: ScopeContext }) {
  const { t } = useTranslation('rum');
  const { application, scope, range, language } = context;
  const browser = isBrowser(application.platform);
  const read = useAggregate(`flag-${flag}`, comparisonQuery(scope, range, flag));
  const variants = React.useMemo(() => readVariants(read.data), [read.data]);
  const tracks = browser ? BROWSER_TRACKS : TRACKS;

  const worst = (value: (variant: Variant) => number | null): number | null => {
    const seen = variants.map(value).filter((number): number is number => number !== null);

    return seen.length > 1 ? Math.max(...seen) : null;
  };

  const worstError = worst((variant) => variant.errorShare);

  return (
    <OverviewCard title={flag} description={t('flags.compare_description')} testId="flags-compare" className="min-w-0">
      <CardState query={read} empty={variants.length === 0} emptyTitle={t('flags.no_values')}>
        <div role="table" aria-label={t('flags.compare_aria', { flag })} className="overflow-x-auto">
          <div role="row" className={HEAD_ROW} style={{ gridTemplateColumns: tracks }}>
            <span role="columnheader">{t('flags.columns.value')}</span>
            <span role="columnheader" className="text-right">{t('flags.columns.sessions')}</span>
            <span role="columnheader" className="text-right">{t('flags.columns.errors')}</span>
            <span role="columnheader" className="text-right">{t('flags.columns.frustration')}</span>
            {browser && (
              <>
                <span role="columnheader" className="text-right">LCP</span>
                <span role="columnheader" className="text-right">INP</span>
                <span role="columnheader" className="text-right">CLS</span>
                <span role="columnheader" />
              </>
            )}
          </div>
          {variants.map((variant) => (
            <div key={variant.value ?? ''} role="row" data-rum-flag-value={variant.value ?? ''} className={BODY_ROW} style={{ gridTemplateColumns: tracks }}>
              <span role="cell" className="min-w-0 truncate font-mono text-tx-0" title={variant.value ?? undefined}>
                {variant.value ?? t('flags.not_evaluated')}
              </span>
              <span role="cell" className="text-right tabular-nums text-tx-1">{formatCount(variant.sessions, language)}</span>
              <span
                role="cell"
                data-rum-flag-errors=""
                className={cn('text-right tabular-nums', worstError !== null && variant.errorShare === worstError ? 'text-[var(--red-ink)]' : 'text-tx-1')}
              >
                {variant.errorShare === null ? '—' : formatShare(variant.errorShare)}
              </span>
              <span role="cell" className="text-right tabular-nums text-tx-1">
                {variant.frustrationShare === null ? '—' : formatShare(variant.frustrationShare)}
              </span>
              {browser && (
                <>
                  <span role="cell" className="text-right tabular-nums text-tx-1">{variant.lcp === null ? '—' : formatDurationNs(variant.lcp)}</span>
                  <span role="cell" className="text-right tabular-nums text-tx-1">{variant.inp === null ? '—' : formatDurationNs(variant.inp)}</span>
                  <span role="cell" className="text-right tabular-nums text-tx-1">{variant.cls === null ? '—' : variant.cls.toFixed(2)}</span>
                  <span role="cell" className="text-right">
                    {variant.value !== null && (
                      <Link
                        data-rum-flag-explore=""
                        to={explorerPath('views', scope, [clause(flagField(flag), variant.value)])}
                        className="font-sans text-xs text-[var(--blue-ink)] outline-none underline-offset-4 hover:underline focus-visible:underline"
                      >
                        {t('flags.open')}
                      </Link>
                    )}
                  </span>
                </>
              )}
            </div>
          ))}
        </div>
      </CardState>
    </OverviewCard>
  );
}
