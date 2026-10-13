import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useParams } from 'react-router-dom';

import { toApiError } from '@/lib/http';
import { activeTimeZone, formatMicrosActive, tzOffsetLabel } from '@/lib/time';
import {
  ReportDocument,
  ReportUnavailable,
  type ReportFact,
} from '@/report-render/Document';
import { useReportRenderState } from '@/report-render/readiness';
import { useAuthStore } from '@/stores/auth';
import { resolveWindow, useTimeStore } from '@/stores/useTimeStore';

import { DashboardRenderer } from '../DashboardRenderer';
import { isDashboardEngineEnabled } from '../featureFlag';
import type { DashboardDefinition, DashboardVariable } from '../schema';
import { useDashboardDefinition } from '../useDashboardDefinition';
import { initialVariableValues } from '../variables';

/**
 * A dashboard as a report prints it: its title, what the panels cover, and the
 * panels.
 *
 * The renderer is told when every panel has its data, and is told why when the
 * dashboard cannot be drawn, so a report is never a picture of a page still
 * loading or of an error.
 */
export function DashboardReport() {
  const { t } = useTranslation('dashboards');
  const { id } = useParams<{ id: string }>();
  const { query, definition, modelError } = useDashboardDefinition(id);
  const failure = query.error ?? modelError;

  if (!isDashboardEngineEnabled()) {
    return <ReportUnavailable reason={t('detail.engine_disabled')} />;
  }
  if (failure) return <ReportUnavailable reason={toApiError(failure).message} />;
  if (!definition) return null;

  // Keyed by dashboard, so one that was ready does not vouch for the next.
  return <DashboardReportDocument key={definition.uid} definition={definition} />;
}

function DashboardReportDocument({ definition }: { definition: DashboardDefinition }) {
  const { t } = useTranslation('dashboards');
  const orgId = useAuthStore((state) => state.ctx?.org_id ?? '');
  const timeWindow = useTimeStore((state) => state.window);
  const [rendered, setRendered] = React.useState<'loading' | 'ready'>('loading');
  const [generatedAt] = React.useState(() => new Date());

  useReportRenderState(rendered);

  const facts = React.useMemo<ReportFact[]>(() => {
    const timezone = activeTimeZone();
    const range = resolveWindow(timeWindow, generatedAt);
    const moment = (date: Date) => formatMicrosActive(date.getTime() * 1_000, false);

    return [
      // A dashboard that hides its time picker does not read the range at all.
      ...(definition.timeSettings.hideTimePicker
        ? []
        : [
            {
              label: t('report.time_range'),
              value: `${moment(range.from)} – ${moment(range.to)}`,
            },
          ]),
      { label: t('report.timezone'), value: timezoneText(timezone) },
      ...variableFacts(definition.variables),
      { label: t('report.generated'), value: moment(generatedAt) },
    ];
  }, [definition.timeSettings.hideTimePicker, definition.variables, generatedAt, t, timeWindow]);

  return (
    <ReportDocument title={definition.title} description={definition.description} facts={facts}>
      <DashboardRenderer
        dashboard={definition}
        orgId={orgId}
        presentation="print"
        // A panel refreshing itself while the page is printed would be printed half loaded.
        refreshIntervalOverride={false}
        onRenderStateChange={setRendered}
      />
    </ReportDocument>
  );
}

/** A zone by its name and its offset, said once when the name is the offset. */
function timezoneText(timezone: string): string {
  const offset = tzOffsetLabel(timezone);

  return offset === timezone ? timezone : `${timezone} · ${offset}`;
}

/**
 * The variables a reader would see above the dashboard, with the values the
 * panels were drawn for. On screen they are controls; here they are stated.
 */
function variableFacts(variables: readonly DashboardVariable[]): ReportFact[] {
  const values = initialVariableValues(variables);

  return variables
    .filter((variable) => variable.hide !== 'variable' && variable.type !== 'constant')
    .map((variable) => ({
      label: variable.label || variable.name,
      value: variableText(values[variable.name]) || '—',
    }));
}

function variableText(value: unknown): string {
  if (Array.isArray(value)) return value.map(variableText).filter(Boolean).join(', ');
  if (value === null || value === undefined) return '';
  return typeof value === 'object' ? JSON.stringify(value) : String(value);
}
