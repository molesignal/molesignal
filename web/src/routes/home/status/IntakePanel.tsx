import { useTranslation } from 'react-i18next';

import { Sparkline } from '@/shell/readout/Sparkline';
import { StatusWord } from '@/shell/StatusWord';
import { formatRelativeMicros } from '@/time/relative';

import { formatBytes, formatCount } from '../format';
import { SIGNAL_TONE } from './model';
import { Panel, PanelNote } from './Panel';
import type { HomeStatus } from './useHomeStatus';

const SIGNAL_TYPES = ['logs', 'metrics', 'traces', 'profiles'] as const;

/** Each signal arriving: its state, what it took in bucket by bucket, how much, and when it last did. */
export function IntakePanel({ status, className }: { status: HomeStatus; className?: string }) {
  const { t, i18n } = useTranslation('onboarding');
  const locale = i18n.resolvedLanguage ?? i18n.language;
  const overview = status.intake.data;
  const signals = Array.isArray(overview?.signals) ? overview.signals : [];

  return (
    <Panel
      title={t('home.intake.title')}
      action={{ label: t('home.intake.streams'), to: '/streams' }}
      className={className}
      testId="home-intake"
    >
      {status.intake.isError ? (
        <PanelNote>{t('home.unavailable')}</PanelNote>
      ) : !overview ? (
        <PanelNote>{t('home.loading')}</PanelNote>
      ) : (
        <>
          <ul className="flex-1">
            {SIGNAL_TYPES.map((type) => {
              const signal = signals.find((candidate) => candidate.streamType === type);
              const state = signal?.status ?? 'no_data';
              return (
                <li
                  key={type}
                  data-home-signal={type}
                  className="grid h-[44px] grid-cols-[minmax(0,1fr)_minmax(0,1.2fr)_64px] items-center gap-3 border-t border-[var(--row-rule)] px-[12px] first:border-t-0"
                >
                  <div className="min-w-0">
                    <div className="truncate font-sans text-sm text-tx-0">{t(`home.signal.${type}`)}</div>
                    <div className="truncate font-sans text-xs text-tx-3">
                      <StatusWord tone={SIGNAL_TONE[state]}>{t(`home.signal_state.${state}`)}</StatusWord>
                      {signal?.lastReceivedAt ? <> · {formatRelativeMicros(signal.lastReceivedAt, locale)}</> : null}
                    </div>
                  </div>
                  <Sparkline
                    values={(signal?.buckets ?? []).map((bucket) => bucket.rows)}
                    variant="bars"
                    className="h-[24px] text-[var(--chart-1)]"
                  />
                  <span className="text-right font-sans text-xs tabular-nums text-tx-1">
                    {signal && signal.rows > 0 ? formatCount(signal.rows) : '—'}
                  </span>
                </li>
              );
            })}
          </ul>
          <footer className="flex items-center justify-between gap-3 border-t border-[var(--row-rule)] px-[12px] py-[8px] font-sans text-xs text-tx-3">
            <span>{t('home.intake.streams_active', { active: overview.activeStreams ?? 0, total: overview.totalStreams ?? 0 })}</span>
            <span className="tabular-nums">
              {t('home.intake.stored', { size: formatBytes(overview.storedBytes) })}
              {overview.intakeBytes != null && <> · {t('home.intake.received', { size: formatBytes(overview.intakeBytes) })}</>}
            </span>
          </footer>
        </>
      )}
    </Panel>
  );
}
