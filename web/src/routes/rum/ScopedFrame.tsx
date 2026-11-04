import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as rumApi from '@/api/rum';
import type { RumApplication } from '@/api/rum';
import { useTimeFormatter } from '@/lib/time';
import { cn } from '@/shell/lib/cn';
import type { TimeWindow } from '@/stores/useTimeStore';
import { TimeRangeControl } from '@/time/TimePicker';

import { byName } from './applications/model';
import { RumFirstRun } from './explorer/FirstRun';
import { useRumTimeWindow } from './explorer/timeWindow';
import type { OverviewRange, OverviewScope } from './overview/model';
import { CONTROL, ScopeRow } from './overview/ScopeRow';
import { useEnvironments, useOverviewRange, useOverviewScope } from './overview/useOverview';
import { RumListPage } from './RumLayout';

/** What a tab that looks at one application is of, and what it needs to ask. */
export interface ScopeContext {
  application: RumApplication;
  scope: OverviewScope;
  /** The range every read is made over, in microseconds. */
  range: OverviewRange;
  language: string;
  clock: ReturnType<typeof useTimeFormatter>;
  /** The range as the reader chose it, and the way to change it. */
  window: TimeWindow;
  setWindow: (window: TimeWindow) => void;
}

/**
 * The frame the tabs that look at one application stand in (performance, frustration): the tabs, the
 * views under them when the tab has several, the application and environment and the range of time
 * they are of. Until the first application exists there is nothing to look at, and the page says how
 * to start. A tab that only some applications can fill says why the others cannot, in place of its
 * body. The head of the page can offer actions of the application in view.
 */
export function ScopedFrame({
  name,
  views,
  unsupported,
  toolbar,
  opensOn,
  children,
}: {
  /** What the body is called in the page: `data-rum-<name>`. */
  name: string;
  /** The views of the tab, as the last row of the navigation card; none where the application has none. */
  views?: ((params: URLSearchParams, application: RumApplication) => React.ReactNode) | undefined;
  /** What stands in the place of the body for an application the tab cannot show, when there is one. */
  unsupported?: ((application: RumApplication) => React.ReactNode) | undefined;
  /** The actions of the head of the page, for the application and range in view and the address. */
  toolbar?: ((context: ScopeContext, params: URLSearchParams) => React.ReactNode) | undefined;
  /** The range the tab opens on when nothing chose one; the day before now when left out. */
  opensOn?: TimeWindow | undefined;
  children: (context: ScopeContext) => React.ReactNode;
}) {
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

  return (
    <Frame applications={byName(applications.data)} name={name} views={views} unsupported={unsupported} toolbar={toolbar} opensOn={opensOn}>
      {children}
    </Frame>
  );
}

function Frame({
  applications,
  name,
  views,
  unsupported,
  toolbar,
  opensOn,
  children,
}: {
  applications: readonly RumApplication[];
  name: string;
  views: ((params: URLSearchParams, application: RumApplication) => React.ReactNode) | undefined;
  unsupported: ((application: RumApplication) => React.ReactNode) | undefined;
  toolbar: ((context: ScopeContext, params: URLSearchParams) => React.ReactNode) | undefined;
  opensOn: TimeWindow | undefined;
  children: (context: ScopeContext) => React.ReactNode;
}) {
  const { i18n } = useTranslation('rum');
  const language = i18n.resolvedLanguage ?? i18n.language;
  const clock = useTimeFormatter();
  const { params, application, environment, setApplication, setEnvironment } = useOverviewScope(applications);
  const { effectiveTimeWindow, setTimeWindow } = useRumTimeWindow(params, opensOn);
  const range = useOverviewRange(effectiveTimeWindow);

  // The list is not empty, and the scope falls back to the first of it.
  const current = application as RumApplication;
  const scope = React.useMemo(() => ({ application: current.id, environment }), [current.id, environment]);
  const environments = useEnvironments(current.id, range);
  const missing = unsupported?.(current) ?? null;
  const context: ScopeContext = { application: current, scope, range, language, clock, window: effectiveTimeWindow, setWindow: setTimeWindow };

  return (
    <RumListPage
      toolbar={missing ? undefined : toolbar?.(context, params)}
      navigationEnd={<TimeRangeControl align="end" className={cn(CONTROL, 'max-w-[220px]')} />}
      navigationScope={
        <ScopeRow
          applications={applications}
          application={current.id}
          environment={environment}
          environments={(environments.data?.values ?? []).map((entry) => entry.value)}
          range={range}
          onApplication={setApplication}
          onEnvironment={setEnvironment}
        />
      }
      navigationViews={missing ? undefined : views?.(params, current)}
    >
      <div {...{ [`data-rum-${name}`]: '' }} data-rum-platform={current.platform} className="space-y-[12px]">
        {missing ?? children(context)}
      </div>
    </RumListPage>
  );
}
