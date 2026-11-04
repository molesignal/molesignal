import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as rumApi from '@/api/rum';
import { FormField, FormInput, FormSelect } from '@/shell/FormDrawer';
import { useAuthStore } from '@/stores/auth';
import type { RumAlertMetric, RumAlertQuery, RumAlertSearch, RumAlertStatistic } from '@/types/alerting';

import { RumCustomFigureFields } from './RumCustomFigureFields';
import { RumQueryFigureFields } from './RumQueryFigureFields';
import {
  emptySearch,
  fitted,
  isAboutIssues,
  isCustomMetric,
  RUM_METRICS,
  RUM_WINDOWS,
  rumDefaultMinSamples,
  rumMetricUnit,
  rumSampleKind,
  rumSupportsPage,
  rumSupportsVersion,
  searchAddsUp,
} from './rumSource';

const DAY_MICROS = 86_400_000_000;

/** The commonest values of a field of an application's rows over the last day, for the choices of a form. */
function useValues(application: string, type: 'sessions' | 'views', field: string, enabled: boolean) {
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const [range] = React.useState(() => {
    const to = Date.now() * 1_000;

    return { from: to - DAY_MICROS, to };
  });

  return useQuery({
    queryKey: ['alerts', 'rum', 'values', orgId, application, type, field],
    queryFn: ({ signal }) =>
      rumApi.facets(
        {
          type,
          from: range.from,
          to: range.to,
          filters: [{ field: 'application.id', op: '=', value: application }],
          field,
          limit: 50,
        },
        signal,
      ),
    enabled: Boolean(orgId && application && enabled),
    staleTime: 60_000,
  });
}

/** The choices of a field: the empty choice, the value the rule has already, then the commonest values. */
function choices(every: string, current: string | null | undefined, seen: readonly string[] | undefined) {
  const values = [...(current ? [current] : []), ...(seen ?? [])];

  return [{ value: '', label: every }, ...[...new Set(values)].map((value) => ({ value, label: value }))];
}

/**
 * Which RUM figure a rule watches: an application, the whole of it or one environment, page and
 * version, and the figure, said with the unit its thresholds are set in. The choices of an
 * environment, a page or a version are the ones the application had over the last day; the one a
 * rule already names stays a choice even when it was not seen. A figure that cannot be narrowed to
 * a page or a version says why, and the minimum of rows it needs is the figure's own unless the
 * rule says.
 */
export function RumSourceFields({
  draft,
  onChange,
  periodSecs,
  onPeriodChange,
  metrics = RUM_METRICS,
}: {
  draft: RumAlertQuery;
  onChange: (next: RumAlertQuery) => void;
  periodSecs: number;
  onPeriodChange: (secs: number) => void;
  /** The figures offered; an anomaly rule watches fewer. */
  metrics?: readonly RumAlertMetric[];
}) {
  const { t } = useTranslation('alerts');

  const applications = useQuery({
    queryKey: ['rum', 'applications'],
    queryFn: () => rumApi.listApplications(),
  });

  const listed = applications.data ?? [];
  const application = listed.find((item) => item.id === draft.applicationId);
  const isApp = application ? application.platform !== 'browser' : false;
  const named = Boolean(draft.applicationId);

  const environments = useValues(draft.applicationId, 'sessions', 'env', named);
  const pages = useValues(draft.applicationId, 'views', isApp ? 'view.name' : 'view.url_path_group', named && rumSupportsPage(draft.metric));
  const versions = useValues(draft.applicationId, 'views', 'version', named && rumSupportsVersion(draft.metric));

  const valuesOf = (found: typeof environments.data) => found?.values.map((item) => item.value);

  const applicationOptions = [
    ...(draft.applicationId && !application ? [{ value: draft.applicationId, label: draft.applicationId }] : []),
    ...[...listed].sort((left, right) => left.name.localeCompare(right.name)).map((item) => ({ value: item.id, label: item.name })),
  ];

  const set = (patch: Partial<RumAlertQuery>) => onChange(fitted({ ...draft, ...patch }));
  const setSearch = (search: RumAlertSearch) => set({ search });

  const issues = isAboutIssues(draft.metric);
  const searched = draft.metric === 'query';
  const pageDisabled = !rumSupportsPage(draft.metric);
  const versionDisabled = !rumSupportsVersion(draft.metric);
  const sampleKind = searched && searchAddsUp(draft.search) ? null : rumSampleKind(draft.metric);
  const defaultSamples = rumDefaultMinSamples(draft.metric);
  const notFor = (what: 'page' | 'version') =>
    searched ? t(`workbench.rum.not_for.${what}_query`) : t(what === 'page' ? (issues ? 'workbench.rum.not_for.page_issues' : 'workbench.rum.not_for.page_sessions') : 'workbench.rum.not_for.version_issues');

  return (
    <div data-alert-rum-source="" className="grid grid-cols-1 gap-4 md:grid-cols-2">
      <FormField label={t('workbench.rum.application')} required>
        <FormSelect
          ariaLabel={t('workbench.rum.application')}
          value={draft.applicationId}
          onChange={(value) => onChange(fitted({ ...draft, applicationId: value, environment: null, page: null, version: null }))}
          options={applicationOptions}
          placeholder={t('workbench.rum.application_placeholder')}
        />
      </FormField>
      <FormField label={t('workbench.rum.environment')}>
        <FormSelect
          ariaLabel={t('workbench.rum.environment')}
          value={draft.environment ?? ''}
          onChange={(value) => set({ environment: value || null })}
          options={choices(t('workbench.rum.every_environment'), draft.environment, valuesOf(environments.data))}
        />
      </FormField>
      <FormField
        label={t(isApp ? 'workbench.rum.screen' : 'workbench.rum.page')}
        {...(pageDisabled ? { hint: notFor('page') } : {})}
      >
        <FormSelect
          ariaLabel={t(isApp ? 'workbench.rum.screen' : 'workbench.rum.page')}
          value={draft.page ?? ''}
          onChange={(value) => set({ page: value || null })}
          options={choices(t(isApp ? 'workbench.rum.every_screen' : 'workbench.rum.every_page'), draft.page, valuesOf(pages.data))}
          disabled={pageDisabled}
        />
      </FormField>
      <FormField
        label={t('workbench.rum.version')}
        {...(versionDisabled ? { hint: notFor('version') } : {})}
      >
        <FormSelect
          ariaLabel={t('workbench.rum.version')}
          value={draft.version ?? ''}
          onChange={(value) => set({ version: value || null })}
          options={choices(t('workbench.rum.every_version'), draft.version, valuesOf(versions.data))}
          disabled={versionDisabled}
        />
      </FormField>
      <FormField
        label={t('workbench.rum.metric')}
        // A query's number is in the unit of its measure, said beside the measure.
        {...(searched ? {} : { hint: t(`workbench.rum.unit.${rumMetricUnit(draft.metric)}`) })}
        required
      >
        <FormSelect
          ariaLabel={t('workbench.rum.metric')}
          value={draft.metric}
          onChange={(value) =>
            set({ metric: value as RumAlertMetric, ...(value === 'query' ? { search: draft.search ?? emptySearch() } : {}) })
          }
          options={metrics.map((metric) => ({ value: metric, label: t(`workbench.rum.metrics.${metric}`) }))}
        />
      </FormField>
      {searched && <RumQueryFigureFields search={draft.search ?? emptySearch()} onChange={setSearch} />}
      {isCustomMetric(draft.metric) && (
        <RumCustomFigureFields
          application={draft.applicationId}
          metric={draft.metric}
          name={draft.customName ?? ''}
          statistic={(draft.statistic ?? 'p75') as RumAlertStatistic}
          onName={(name) => set({ customName: name || null })}
          onStatistic={(statistic) => set({ statistic: statistic === 'p75' ? null : statistic })}
        />
      )}
      <FormField label={t('workbench.rum.window')}>
        <FormSelect
          ariaLabel={t('workbench.rum.window')}
          value={String(periodSecs)}
          onChange={(value) => onPeriodChange(Number(value))}
          options={RUM_WINDOWS.map((secs) => ({ value: String(secs), label: t(`workbench.rum.windows.${secs}`) }))}
        />
      </FormField>
      {sampleKind && draft.statistic !== 'count' && (
        <FormField
          label={t('workbench.rum.min_samples')}
          hint={t('workbench.rum.min_samples_hint', { rows: defaultSamples, kind: t(`workbench.rum.sample_kinds.${sampleKind}`) })}
        >
          <FormInput
            type="number"
            min={0}
            step={10}
            inputMode="numeric"
            aria-label={t('workbench.rum.min_samples')}
            value={draft.minSamples ?? ''}
            placeholder={String(defaultSamples)}
            onChange={(event) => {
              const number = Number(event.target.value.trim());

              set({ minSamples: event.target.value.trim() === '' || !Number.isFinite(number) ? null : Math.max(0, Math.round(number)) });
            }}
          />
        </FormField>
      )}
    </div>
  );
}
