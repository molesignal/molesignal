import { useQuery } from '@tanstack/react-query';
import { AppWindow, Monitor, MonitorSmartphone } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as rumApi from '@/api/rum';
import type { RumApplication } from '@/api/rum';
import { widenTimeWindow } from '@/investigation/timeRangeRecovery';
import { ProductState } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { TimeRangeControl } from '@/time/TimePicker';
import { usePreloadWorldMap } from '@/viz/worldmap/preload';

import { RumDashboardLink } from './DashboardLink';
import { FrustrationCard } from './Frustration';
import { IssuesCard } from './Issues';
import {
  countriesQuery,
  frustrationQuery,
  isBrowser,
  pagesQuery,
  pulseQuery,
  readCountries,
  readPulse,
  readVitals,
  sharesQuery,
  vitalsQuery,
} from './model';
import { OriginsCard } from './Origins';
import { CardState } from './OverviewCard';
import { PagesCard } from './Pages';
import { OverviewPulse } from './Pulse';
import { CONTROL, ScopeRow } from './ScopeRow';
import { SharesCard } from './Shares';
import { RumInvestigateButton } from '../agent/InvestigateButton';
import { byName } from '../applications/model';
import { HEAD_ACTION } from '../headActions';
import { RumGeoCredit, RumListPage } from '../RumLayout';
import { useAggregate, useEnvironments, useOverviewRange, useOverviewScope } from './useOverview';
import { RumFirstRun } from '../explorer/FirstRun';
import { useRumTimeWindow } from '../explorer/timeWindow';

/**
 * The landing of RUM: how one application did over the time in view. One statement says how many
 * sessions there were, of how many people, and how many had no error, with the Core Web Vitals
 * graded under it; then the pages and where the visitors were, what they used, and what
 * frustrated them. Every figure leads to the rows it counts, in the explorer.
 *
 * Until the first application exists there is nothing to look at, and the page says how to start.
 */
export function RumOverview() {
  const applications = useQuery({
    queryKey: ['rum', 'applications'],
    queryFn: () => rumApi.listApplications(),
  });

  if (applications.isPending) return <RumListPage state={{ variant: 'loading' }} />;

  if (applications.isError) return <RumListPage state={{ variant: 'error', error: applications.error }} />;

  if (applications.data.length === 0) {
    return (
      <RumListPage>
        <RumFirstRun embedded />
      </RumListPage>
    );
  }

  return <OverviewPage applications={byName(applications.data)} />;
}

function OverviewPage({ applications }: { applications: readonly RumApplication[] }) {
  const { t, i18n } = useTranslation('rum');
  const language = i18n.resolvedLanguage ?? i18n.language;
  const { params, application, environment, setApplication, setEnvironment } = useOverviewScope(applications);
  const { effectiveTimeWindow, setTimeWindow } = useRumTimeWindow(params);
  const range = useOverviewRange(effectiveTimeWindow);

  // The map of where the sessions came from is on this page: its shapes load while the numbers do.
  usePreloadWorldMap();

  // The list is not empty, and the scope falls back to the first of it.
  const current = application as RumApplication;
  const browser = isBrowser(current.platform);
  const scope = React.useMemo(() => ({ application: current.id, environment }), [current.id, environment]);

  const environments = useEnvironments(current.id, range);
  const pulse = useAggregate('pulse', pulseQuery(scope, range));
  const vitals = useAggregate('vitals', browser ? vitalsQuery(scope, range) : null);
  const pages = useAggregate('pages', pagesQuery(scope, range, current.platform));
  const countries = useAggregate('countries', countriesQuery(scope, range));
  const browsers = useAggregate('browsers', browser ? sharesQuery(scope, range, 'browser.name') : null);
  const systems = useAggregate('systems', sharesQuery(scope, range, 'os.name'));
  const devices = useAggregate('devices', sharesQuery(scope, range, 'device.type'));
  const frustration = useAggregate('frustration', frustrationQuery(scope, range, current.platform));

  const numbers = readPulse(pulse.data);
  // The card stays while the countries are being read and when they cannot be counted; it goes
  // when the sessions have none.
  const hasMap = !countries.isSuccess || Boolean(countries.data?.unavailable) || readCountries(countries.data).length > 0;

  const scopeRow = (
    <ScopeRow
      applications={applications}
      application={current.id}
      environment={environment}
      environments={(environments.data?.values ?? []).map((entry) => entry.value)}
      range={range}
      onApplication={setApplication}
      onEnvironment={setEnvironment}
    />
  );

  const body = (
    <CardState query={pulse} empty={false} emptyTitle="">
      {numbers && (
        <>
          <OverviewPulse pulse={numbers} vitals={browser ? readVitals(vitals.data) : null} scope={scope} language={language} />
          <div className={cn('grid min-w-0 gap-[12px]', hasMap && 'xl:grid-cols-[minmax(0,1.5fr)_minmax(0,1fr)]')}>
            <PagesCard query={pages} scope={scope} browser={browser} language={language} />
            {hasMap && <OriginsCard query={countries} total={numbers.sessions} scope={scope} language={language} />}
          </div>
          <div className="grid min-w-0 gap-[12px] [grid-template-columns:repeat(auto-fit,minmax(min(280px,100%),1fr))]">
            {browser && (
              <SharesCard
                kind="browsers"
                field="browser.name"
                icon={AppWindow}
                tone="text-[var(--chart-1)]"
                query={browsers}
                total={numbers.sessions}
                scope={scope}
                language={language}
              />
            )}
            <SharesCard
              kind="systems"
              field="os.name"
              icon={Monitor}
              tone="text-[var(--chart-6)]"
              query={systems}
              total={numbers.sessions}
              scope={scope}
              language={language}
            />
            <SharesCard
              kind="devices"
              field="device.type"
              icon={MonitorSmartphone}
              tone="text-[var(--chart-4)]"
              query={devices}
              total={numbers.sessions}
              scope={scope}
              language={language}
            />
          </div>
          <div className="grid min-w-0 gap-[12px] xl:grid-cols-[minmax(0,1.5fr)_minmax(0,1fr)]">
            <IssuesCard scope={scope} range={range} language={language} />
            <FrustrationCard query={frustration} scope={scope} browser={browser} language={language} />
          </div>
          {hasMap && <RumGeoCredit />}
        </>
      )}
    </CardState>
  );

  // The dashboard of the application, and Mole Agent on how it did: what the head of the page offers.
  const toolbar = (
    <>
      <RumDashboardLink application={current} environment={environment} className={HEAD_ACTION} />
      <RumInvestigateButton subject={{ application: { id: current.id, name: current.name }, environment, range }} />
    </>
  );

  return (
    <RumListPage
      toolbar={toolbar}
      navigationEnd={<TimeRangeControl align="end" className={cn(CONTROL, 'max-w-[220px]')} />}
      navigationScope={scopeRow}
    >
      <div data-rum-overview="" data-rum-platform={current.platform} className="space-y-[12px]">
        {numbers?.sessions === 0 ? (
          <ProductState
            variant="empty"
            title={t('overview.empty.title')}
            description={t('overview.empty.description')}
            action={
              <ChromeButton onClick={() => setTimeWindow(widenTimeWindow(effectiveTimeWindow))}>
                {t('overview.empty.widen')}
              </ChromeButton>
            }
          />
        ) : (
          body
        )}
      </div>
    </RumListPage>
  );
}
