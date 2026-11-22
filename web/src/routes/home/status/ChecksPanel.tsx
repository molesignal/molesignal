import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { Sparkline } from '@/shell/readout/Sparkline';
import { StatusWord } from '@/shell/StatusWord';

import { CHECK_TONE, formatShare } from './model';
import { Panel, PanelNote } from './Panel';
import type { HomeStatus } from './useHomeStatus';

const CHECK_ROWS = 4;

/** How the checks did over the window, and the ones not passing now. */
export function ChecksPanel({ status, className }: { status: HomeStatus; className?: string }) {
  const { t } = useTranslation('onboarding');
  const summary = status.summaries.checks;
  const availability = status.availability.data;

  return (
    <Panel
      title={t('home.checks.title')}
      action={{ label: t('home.view_all'), to: '/synthetics/checks' }}
      className={className}
      testId="home-checks"
    >
      {status.monitors.isError ? (
        <PanelNote>{t('home.unavailable')}</PanelNote>
      ) : !summary ? (
        <PanelNote>{t('home.loading')}</PanelNote>
      ) : summary.active === 0 ? (
        <PanelNote testId="home-checks-empty">
          {t('home.checks.empty')}{' '}
          <Link to="/synthetics/checks" className="font-strong text-[var(--blue-ink)] hover:underline">
            {t('home.checks.create')}
          </Link>
        </PanelNote>
      ) : (
        <>
          <div className="grid grid-cols-[auto_minmax(0,1fr)] items-end gap-4 px-[12px] pb-[8px] pt-[10px]">
            <div>
              <div className="font-sans text-xs text-tx-2">{t('home.checks.availability')}</div>
              <div data-home-availability className="type-page-title font-sans font-display tabular-nums text-tx-0">
                {formatShare(availability?.total?.availability)}
              </div>
            </div>
            <Sparkline
              values={(Array.isArray(availability?.buckets) ? availability.buckets : []).map((bucket) => bucket.availability)}
              className="h-[32px] text-[var(--chart-1)]"
            />
          </div>
          {summary.attention.length === 0 ? (
            <PanelNote>{t('home.checks.all_passing', { count: summary.active })}</PanelNote>
          ) : (
            <ul className="flex-1">
              {summary.attention.slice(0, CHECK_ROWS).map((monitor) => (
                <li
                  key={monitor.id}
                  data-home-check={monitor.id}
                  className="relative flex h-[36px] items-center gap-3 border-t border-[var(--row-rule)] px-[12px] hover:bg-[var(--bg-hover)]"
                >
                  <Link
                    to={`/synthetics/checks/${encodeURIComponent(monitor.id)}`}
                    className="min-w-0 flex-1 truncate font-sans text-sm text-tx-0 after:absolute after:inset-0"
                  >
                    {monitor.name}
                  </Link>
                  <StatusWord tone={CHECK_TONE[monitor.state]} className="text-xs">
                    {t(`home.check_state.${monitor.state}`)}
                  </StatusWord>
                </li>
              ))}
            </ul>
          )}
        </>
      )}
    </Panel>
  );
}
