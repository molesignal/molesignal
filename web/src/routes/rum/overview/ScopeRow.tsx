import { useTranslation } from 'react-i18next';

import type { RumApplication } from '@/api/rum';
import { cn } from '@/shell/lib/cn';
import { FittedSelect } from '@/shell/ui/fitted-select';

import { EstimateNote } from './EstimateNote';
import type { OverviewRange } from './model';
import { TestTrafficNote } from './TestTrafficNote';
import { RumAlertsButton } from '../alerts/AlertsButton';

const ALL = '__all__';

/** A control of the navigation card: a field in the fill of its ground, a step darker under the pointer. */
export const CONTROL =
  'h-8 border-0 bg-[var(--control-surface)] px-2.5 text-xs hover:bg-bg-3 focus-visible:bg-bg-3 data-[state=open]:bg-bg-3';

/**
 * What a page of the module is scoped to, as the second row of its navigation card: the
 * application, and the environment of it or all of them. The environments are the ones the
 * application has sessions in over the range; a range that has none of the one named in the address
 * still shows it chosen. At the end of the row stand a note that the sessions tests drove are left out,
 * when there are any, a note that the totals are estimates, when the application keeps only a share of
 * its sessions, and the alerts of the application.
 */
export function ScopeRow({
  applications,
  application,
  environment,
  environments,
  range,
  onApplication,
  onEnvironment,
}: {
  applications: readonly RumApplication[];
  /** The id of the application the page is of. */
  application: string;
  environment: string;
  environments: readonly string[];
  /** What the page is read over; the note of an application that keeps a share of its sessions is of it. */
  range: OverviewRange;
  onApplication: (id: string) => void;
  onEnvironment: (name: string) => void;
}) {
  const { t, i18n } = useTranslation('rum');
  const names = environment && !environments.includes(environment) ? [...environments, environment] : environments;
  const current = applications.find((candidate) => candidate.id === application);

  return (
    <>
      <FittedSelect
        aria-label={t('overview.scope.application')}
        data-rum-scope="application"
        value={application}
        onValueChange={onApplication}
        options={applications.map((candidate) => ({ value: candidate.id, label: candidate.name }))}
        maxWidth={240}
        prefix={t('overview.scope.application')}
        className={cn(CONTROL, 'font-strong text-tx-1')}
      />
      <FittedSelect
        aria-label={t('overview.scope.environment')}
        data-rum-scope="environment"
        value={environment || ALL}
        onValueChange={(value) => onEnvironment(value === ALL ? '' : value)}
        options={[{ value: ALL, label: t('overview.scope.all_environments') }, ...names.map((name) => ({ value: name, label: name }))]}
        maxWidth={200}
        prefix={environment ? t('overview.scope.environment') : undefined}
        className={cn(CONTROL, 'font-strong text-tx-1')}
      />
      {current && (
        <div className="ml-auto flex shrink-0 items-center gap-2">
          <TestTrafficNote application={current.id} environment={environment} range={range} language={i18n.resolvedLanguage ?? i18n.language} />
          <EstimateNote application={current.id} environment={environment} range={range} />
          <RumAlertsButton application={current} environment={environment} className={CONTROL} />
        </div>
      )}
    </>
  );
}
