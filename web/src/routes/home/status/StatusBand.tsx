import type * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { useTimeFormatter } from '@/lib/time';
import { formatCount, formatDuration } from '@/routes/apm/format';
import { Sparkline } from '@/shell/readout/Sparkline';
import { SpectrumBar } from '@/shell/readout/SpectrumBar';
import { StatusWord } from '@/shell/StatusWord';

import { apmTrend, formatShare, SEVERITY_FILL, SEVERITY_ORDER, SIGNAL_TONE, verdictOf, type HomeWindow } from './model';
import type { HomeStatus } from './useHomeStatus';

const SIGNAL_TYPES = ['logs', 'metrics', 'traces', 'profiles'] as const;

/**
 * The state of the system on one strip: a verdict, the signals arriving, and a tile for each thing
 * a responder watches. Each tile opens the page it summarises.
 */
export function StatusBand({ status, period }: { status: HomeStatus; period: HomeWindow }) {
  const { t, i18n } = useTranslation('onboarding');
  const formatter = useTimeFormatter();
  const clock = new Intl.DateTimeFormat(i18n.resolvedLanguage ?? i18n.language, {
    timeZone: formatter.tz,
    hour: '2-digit',
    minute: '2-digit',
    hour12: formatter.format === 'local_12h',
  });
  const { allowed, summaries, apm, intake } = status;
  const overview = apm.data;
  const hasTraffic = (overview?.red?.requestCount ?? 0) > 0;
  const trend = apmTrend(overview);
  const signals = Array.isArray(intake.data?.signals) ? intake.data.signals : undefined;
  const verdict = verdictOf({
    incidents: summaries.incidents,
    checks: summaries.checks,
    serviceHealth: hasTraffic ? overview?.serviceHealth : undefined,
    signals,
  });
  const loaded = [
    !allowed.incidents || status.incidents.data,
    !allowed.checks || status.monitors.data,
    !allowed.services || status.apm.data,
    !allowed.intake || status.intake.data,
  ].every(Boolean);
  const generatedAt = intake.data?.generatedAt ?? status.range.to;

  const incidents = summaries.incidents;
  const rules = summaries.rules;
  const checks = summaries.checks;
  const health = overview?.serviceHealth;
  const serviceTotal = health ? health.healthy + health.warning + health.critical + health.no_traffic : 0;

  return (
    <section
      data-testid="home-status-band"
      aria-label={t('home.status.title')}
      className="overflow-hidden rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]"
    >
      <header className="flex min-h-[40px] flex-wrap items-center gap-x-3 gap-y-1 border-b border-[var(--row-rule)] px-[12px] py-[6px]">
        <h2 className="font-sans text-md font-display text-tx-0">{t('home.status.title')}</h2>
        <span data-testid="home-verdict" data-tone={!(allowed.incidents || allowed.rules || allowed.services || allowed.checks || allowed.intake) ? 'none' : loaded ? verdict.tone : 'loading'} className="font-sans text-sm">
          {!(allowed.incidents || allowed.rules || allowed.services || allowed.checks || allowed.intake) ? (
            <StatusWord tone="dim">{t('home.status.nothing')}</StatusWord>
          ) : !loaded ? (
            <StatusWord tone="dim">{t('home.status.loading')}</StatusWord>
          ) : verdict.issues === 0 ? (
            <StatusWord tone="green" strong>
              {t('home.status.ok')}
            </StatusWord>
          ) : (
            <StatusWord tone={verdict.tone} strong>
              {t('home.status.issues', { count: verdict.issues })}
            </StatusWord>
          )}
        </span>
        <span className="font-sans text-xs text-tx-3">
          {t(`home.window.${period}`)} · {t('home.status.updated', { time: clock.format(new Date(generatedAt / 1000)) })}
        </span>
        {signals && (
          <ul aria-label={t('home.intake.title')} className="ml-auto flex flex-wrap items-center gap-x-3 font-sans text-xs text-tx-2">
            {SIGNAL_TYPES.map((type) => {
              const signal = signals.find((candidate) => candidate.streamType === type);
              const state = signal?.status ?? 'no_data';
              return (
                <li key={type} data-signal={type} data-state={state} className="flex items-center gap-1.5">
                  <SignalDot tone={SIGNAL_TONE[state]} />
                  {t(`home.signal.${type}`)}
                  {state !== 'healthy' && <span className="text-tx-3">{t(`home.signal_state.${state}`)}</span>}
                </li>
              );
            })}
          </ul>
        )}
      </header>
      <div className="grid grid-cols-[repeat(auto-fit,minmax(150px,1fr))] gap-px bg-[var(--row-rule)]">
        {allowed.incidents && (
          <Tile
            id="incidents"
            to="/alerts/incidents"
            label={t('home.tiles.incidents')}
            value={incidents ? String(incidents.active.length) : '—'}
            valueTone={
              !incidents || incidents.active.length === 0
                ? undefined
                : incidents.bySeverity.critical + incidents.bySeverity.error > 0
                  ? 'red'
                  : 'yellow'
            }
            sub={
              incidents && incidents.active.length > 0
                ? SEVERITY_ORDER.filter((severity) => incidents.bySeverity[severity] > 0)
                    .map((severity) => `${t(`home.severity.${severity}`)} ${incidents.bySeverity[severity]}`)
                    .concat(incidents.unacknowledged > 0 ? [t('home.tiles.unacknowledged', { count: incidents.unacknowledged })] : [])
                    .join(' · ')
                : t('home.tiles.no_incidents')
            }
            foot={
              <SpectrumBar
                label={t('home.tiles.incidents')}
                legend={false}
                segments={SEVERITY_ORDER.map((severity) => ({
                  key: severity,
                  label: t(`home.severity.${severity}`),
                  value: incidents?.bySeverity[severity] ?? 0,
                  color: SEVERITY_FILL[severity],
                }))}
              />
            }
          />
        )}
        {allowed.rules && (
          <Tile
            id="rules"
            to="/alerts/rules"
            label={t('home.tiles.rules')}
            value={rules ? String(rules.firing) : '—'}
            valueTone={rules && rules.firing > 0 ? 'red' : undefined}
            unit={rules ? t('home.tiles.of_enabled', { count: rules.enabled }) : undefined}
            sub={rules ? t('home.tiles.pending', { count: rules.pending }) : ''}
            foot={
              <SpectrumBar
                label={t('home.tiles.rules')}
                legend={false}
                segments={[
                  { key: 'firing', label: t('home.rule_state.firing'), value: rules?.firing ?? 0, color: 'var(--red)' },
                  { key: 'pending', label: t('home.rule_state.pending'), value: rules?.pending ?? 0, color: 'var(--yellow)' },
                  {
                    key: 'ok',
                    label: t('home.rule_state.ok'),
                    value: rules ? rules.enabled - rules.firing - rules.pending : 0,
                    color: 'var(--green)',
                  },
                ]}
              />
            }
          />
        )}
        {allowed.services && (
          <Tile
            id="services"
            to="/apm/services"
            label={t('home.tiles.services')}
            value={hasTraffic && health ? String(health.healthy) : '—'}
            unit={hasTraffic ? t('home.tiles.of_total', { count: serviceTotal }) : undefined}
            sub={
              hasTraffic && health
                ? [
                    `${t('home.service_health.critical')} ${health.critical}`,
                    `${t('home.service_health.warning')} ${health.warning}`,
                    `${t('home.service_health.no_traffic')} ${health.no_traffic}`,
                  ].join(' · ')
                : t('home.tiles.no_traces')
            }
            foot={
              <SpectrumBar
                label={t('home.tiles.services')}
                legend={false}
                segments={[
                  { key: 'critical', label: t('home.service_health.critical'), value: hasTraffic ? (health?.critical ?? 0) : 0, color: 'var(--red)' },
                  { key: 'warning', label: t('home.service_health.warning'), value: hasTraffic ? (health?.warning ?? 0) : 0, color: 'var(--yellow)' },
                  { key: 'healthy', label: t('home.service_health.healthy'), value: hasTraffic ? (health?.healthy ?? 0) : 0, color: 'var(--green)' },
                ]}
              />
            }
          />
        )}
        {allowed.checks && (
          <Tile
            id="checks"
            to="/synthetics/checks"
            label={t('home.tiles.checks')}
            value={checks && checks.active > 0 ? String(checks.byState.healthy) : '—'}
            unit={checks && checks.active > 0 ? t('home.tiles.of_passing', { count: checks.active }) : undefined}
            valueTone={checks && checks.byState.failing > 0 ? 'red' : undefined}
            sub={
              checks && checks.active > 0
                ? [
                    `${t('home.check_state.failing')} ${checks.byState.failing}`,
                    `${t('home.check_state.degraded')} ${checks.byState.degraded}`,
                    `${t('home.check_state.flaky')} ${checks.byState.flaky}`,
                  ].join(' · ')
                : t('home.tiles.no_checks')
            }
            foot={
              <SpectrumBar
                label={t('home.tiles.checks')}
                legend={false}
                segments={[
                  { key: 'failing', label: t('home.check_state.failing'), value: checks?.byState.failing ?? 0, color: 'var(--red)' },
                  { key: 'degraded', label: t('home.check_state.degraded'), value: checks?.byState.degraded ?? 0, color: 'var(--yellow)' },
                  { key: 'flaky', label: t('home.check_state.flaky'), value: checks?.byState.flaky ?? 0, color: 'var(--orange)' },
                  { key: 'healthy', label: t('home.check_state.healthy'), value: checks?.byState.healthy ?? 0, color: 'var(--green)' },
                ]}
              />
            }
          />
        )}
        {allowed.services && (
          <Tile
            id="error_rate"
            to="/apm/overview"
            label={t('home.tiles.error_rate')}
            value={hasTraffic ? formatShare(overview?.red?.errorRate) : '—'}
            sub={hasTraffic ? t('home.tiles.requests', { value: formatCount(overview?.red?.requestCount ?? 0) }) : t('home.tiles.no_traces')}
            foot={<Sparkline values={trend.errorRate} zeroBased className="h-[28px] text-[var(--chart-1)]" />}
          />
        )}
        {allowed.services && (
          <Tile
            id="p95"
            to="/apm/overview"
            label={t('home.tiles.p95')}
            value={hasTraffic ? formatDuration(overview?.red?.p95Time) : '—'}
            sub={
              hasTraffic
                ? `P50 ${formatDuration(overview?.red?.p50Time)} · P99 ${formatDuration(overview?.red?.p99Time)}`
                : t('home.tiles.no_traces')
            }
            foot={<Sparkline values={trend.p95} className="h-[28px] text-[var(--chart-1)]" />}
          />
        )}
      </div>
    </section>
  );
}

function Tile({
  id,
  to,
  label,
  value,
  valueTone,
  unit,
  sub,
  foot,
}: {
  id: string;
  to: string;
  label: string;
  value: string;
  valueTone?: 'red' | 'yellow' | undefined;
  unit?: string | undefined;
  sub: string;
  foot: React.ReactNode;
}) {
  return (
    <Link
      to={to}
      data-home-tile={id}
      className="group flex min-w-0 flex-col bg-[var(--functional-surface)] px-[12px] pb-[10px] pt-[10px] transition-colors duration-fast hover:bg-[var(--bg-hover)] focus-visible:bg-[var(--bg-hover)]"
    >
      <span className="truncate font-sans text-xs text-tx-2">{label}</span>
      <span className="mt-1 flex min-w-0 items-baseline gap-1">
        <span
          data-home-tile-value
          className={
            'type-page-title font-sans font-display tabular-nums ' +
            (valueTone === 'red' ? 'text-[var(--red-ink)]' : valueTone === 'yellow' ? 'text-[var(--yellow-ink)]' : 'text-tx-0')
          }
        >
          {value}
        </span>
        {unit && <span className="truncate font-sans text-xs text-tx-2">{unit}</span>}
      </span>
      <span className="mt-0.5 truncate font-sans text-xs tabular-nums text-tx-3">{sub || ' '}</span>
      <span className="mt-2 flex h-[28px] items-end">
        <span className="w-full">{foot}</span>
      </span>
    </Link>
  );
}

function SignalDot({ tone }: { tone: string }) {
  const color =
    tone === 'green' ? 'var(--green)' : tone === 'yellow' ? 'var(--yellow)' : tone === 'red' ? 'var(--red)' : 'var(--tx-4)';
  return <span aria-hidden="true" className="h-[6px] w-[6px] shrink-0 rounded-full" style={{ background: color }} />;
}
