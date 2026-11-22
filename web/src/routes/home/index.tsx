import { RefreshCw, Rocket } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useSearchParams } from 'react-router-dom';

import { OverviewPage } from '@/product/templates';
import { ChromeButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { FittedSelect } from '@/shell/ui/fitted-select';

import { QuickStartDrawer } from './QuickStartDrawer';
import { ChecksPanel } from './status/ChecksPanel';
import { IncidentsPanel } from './status/IncidentsPanel';
import { IntakePanel } from './status/IntakePanel';
import { HOME_WINDOWS, type HomeWindow } from './status/model';
import { OnCallPanel } from './status/OnCallPanel';
import { ServicesPanel } from './status/ServicesPanel';
import { StatusBand } from './status/StatusBand';
import { useHomeStatus } from './status/useHomeStatus';
import { useActivation } from './useActivation';

function parseWindow(raw: string | null): HomeWindow {
  return HOME_WINDOWS.includes(raw as HomeWindow) ? (raw as HomeWindow) : '1h';
}

/**
 * Home: how the system stands now. A strip of the figures a responder watches, then the services
 * to look at first and the incidents going on, then the signals arriving, the checks and the
 * reader's on-call. Each part shows only to whoever can open the page it comes from.
 */
export function Home() {
  const { t } = useTranslation('onboarding');
  const nav = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();
  const period = parseWindow(searchParams.get('window'));
  const status = useHomeStatus(period);
  const activation = useActivation();
  const [quickStartOpen, setQuickStartOpen] = React.useState(false);
  const remaining = activation.todo?.remaining ?? 0;
  const { allowed } = status;

  const setWindow = (next: string) => {
    setSearchParams(
      (params) => {
        const updated = new URLSearchParams(params);
        if (next === '1h') updated.delete('window');
        else updated.set('window', next);
        return updated;
      },
      { replace: true },
    );
  };

  return (
    <OverviewPage
      title={t('home.title')}
      appearance="surface"
      headerCompact
      bodyClassName="pb-[24px] pt-0"
      toolbar={
        <div className="flex items-center gap-2">
          {remaining > 0 && (
            <span data-home-surface="activation">
              <ChromeButton
                className="h-9"
                aria-label={t('home.quick_start', { count: remaining })}
                onClick={() => setQuickStartOpen(true)}
              >
                <Rocket aria-hidden="true" className="h-3.5 w-3.5" />
                <span className="hidden sm:inline">{t('home.quick_start', { count: remaining })}</span>
              </ChromeButton>
            </span>
          )}
          <FittedSelect
            aria-label={t('home.window.label')}
            data-testid="home-window"
            value={period}
            onValueChange={setWindow}
            maxWidth={160}
            options={HOME_WINDOWS.map((value) => ({ value, label: t(`home.window.${value}`) }))}
          />
          <ChromeButton
            className="h-9 w-9 justify-center px-0"
            aria-label={t('home.refresh')}
            title={t('home.refresh')}
            onClick={status.refresh}
            data-testid="home-refresh"
          >
            <RefreshCw aria-hidden="true" className={cn('h-3.5 w-3.5', status.fetching && 'animate-spin')} />
          </ChromeButton>
        </div>
      }
    >
      <div className="mx-auto flex w-full max-w-[2200px] flex-col gap-[8px]" data-testid="home">
        <StatusBand status={status} period={period} />
        {(allowed.services || allowed.incidents) && (
          <div className="grid grid-cols-1 gap-[8px] xl:grid-cols-12">
            {allowed.services && (
              <ServicesPanel status={status} className={allowed.incidents ? 'xl:col-span-8' : 'xl:col-span-12'} />
            )}
            {allowed.incidents && (
              <IncidentsPanel status={status} className={allowed.services ? 'xl:col-span-4' : 'xl:col-span-12'} />
            )}
          </div>
        )}
        <div className="grid grid-cols-1 gap-[8px] md:grid-cols-2 xl:grid-cols-3">
          {allowed.intake && <IntakePanel status={status} />}
          {allowed.checks && <ChecksPanel status={status} />}
          <OnCallPanel />
        </div>
      </div>
      <QuickStartDrawer
        open={quickStartOpen}
        onOpenChange={setQuickStartOpen}
        state={activation.state}
        onOpenStep={(to) => {
          setQuickStartOpen(false);
          nav(to);
        }}
        onLoadSample={() =>
          activation.loadSample.mutate(undefined, {
            onSuccess: (result) => {
              if (result.dashboardId) nav(`/dashboards/${encodeURIComponent(result.dashboardId)}`);
            },
          })
        }
        loadingSample={activation.loadSample.isPending}
      />
    </OverviewPage>
  );
}
