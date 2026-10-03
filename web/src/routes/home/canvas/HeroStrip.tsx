import type * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { HomeOverview } from '@/api/home';

import { formatByteRate, formatBytesCompact, formatCount } from './format';
import { HealthHero, type HealthHeroActions } from './HealthHero';
import { MetricCard, type MetricTone } from './MetricCard';
import type { FleetSummary } from './streamCondition';

function CompressionDetail({ overview }: { overview: HomeOverview | undefined }) {
  const { t } = useTranslation('onboarding');
  if (!overview) return <>{t('home.loading')}</>;
  const ratio = overview.compression_savings_ratio;
  if (ratio == null) return <>{t('home.kpis.compression_pending')}</>;
  if (ratio >= 0) {
    return <>{t('home.kpis.compression_saved', { percent: (ratio * 100).toFixed(1) })}</>;
  }
  return (
    <>
      {t('home.kpis.compression_overhead', {
        percent: (Math.abs(ratio) * 100).toFixed(1),
      })}
    </>
  );
}

function alertTone(firing: number, acknowledged: number): MetricTone | undefined {
  if (firing > 0) return 'red';
  if (acknowledged > 0) return 'yellow';
  return undefined;
}

/**
 * The first band of Home: the health verdict on the left, three secondary
 * readouts (alerts, intake, stored) on the right. Stream health used to be
 * spread over three equal cards; it is now one statement.
 */
export function HeroStrip({
  fleet,
  fleetLoading,
  overview,
  overviewLoading,
  firing,
  acknowledged,
  incidentsLoading,
  windowLabel,
  lastReceived,
  actions,
  heroDecoration,
  onOpenAlerts,
  onOpenStreams,
}: {
  fleet: FleetSummary | undefined;
  fleetLoading: boolean;
  overview: HomeOverview | undefined;
  overviewLoading: boolean;
  firing: number;
  acknowledged: number;
  incidentsLoading: boolean;
  windowLabel: string;
  lastReceived: string;
  actions: HealthHeroActions;
  heroDecoration?: React.ReactNode;
  onOpenAlerts: () => void;
  onOpenStreams: () => void;
}) {
  const { t } = useTranslation('onboarding');

  return (
    <section aria-label={t('home.kpis.label')} className="home-canvas-hero-strip">
      <HealthHero
        fleet={fleet}
        loading={fleetLoading}
        lastReceived={lastReceived}
        actions={actions}
        decoration={heroDecoration}
        riseIndex={0}
      />
      <div className="home-canvas-metrics">
        <MetricCard
          label={t('home.kpis.active_alerts')}
          value={String(firing + acknowledged)}
          detail={t('home.kpis.alert_detail', { firing, acknowledged })}
          tone={alertTone(firing, acknowledged)}
          loading={incidentsLoading}
          riseIndex={1}
          onClick={onOpenAlerts}
        />
        <MetricCard
          label={t('home.kpis.intake_bytes')}
          scope={windowLabel}
          value={formatBytesCompact(overview?.intake_bytes)}
          detail={
            overview
              ? t('home.kpis.intake_detail', {
                  rate: formatByteRate(overview.intake_bytes, overview.window?.window_secs ?? 1),
                  rows: formatCount(overview.rows),
                })
              : t('home.loading')
          }
          loading={overviewLoading}
          riseIndex={2}
          onClick={onOpenStreams}
        />
        <MetricCard
          label={t('home.kpis.stored_bytes')}
          scope={windowLabel}
          value={formatBytesCompact(overview?.stored_bytes)}
          detail={<CompressionDetail overview={overview} />}
          loading={overviewLoading}
          riseIndex={3}
          onClick={onOpenStreams}
        />
      </div>
    </section>
  );
}
