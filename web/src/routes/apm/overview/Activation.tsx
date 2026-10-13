import { ArrowRight } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { TraceArt } from '@/shell/firstRun/art';
import { FirstRun } from '@/shell/firstRun/FirstRun';

/** The APM overview before any backend trace has arrived: what it will show and how to start. */
export function ApmActivation() {
  const { t } = useTranslation('apm');

  return (
    <FirstRun
      data-testid="apm-activation"
      eyebrow={t('first_run.eyebrow')}
      title={t('states.activation_empty')}
      description={t('states.activation_empty_description')}
      illustration={<TraceArt />}
      actions={
        <>
          <Link
            to="/datasource/applications/opentelemetry?signal=traces"
            className="ms-press inline-flex h-9 items-center gap-2 rounded-md bg-indigo px-3 font-sans text-sm font-bold text-white shadow-primary hover:brightness-90 focus-visible:brightness-90"
          >
            {t('first_run.connect')}
            <ArrowRight className="h-3.5 w-3.5" aria-hidden="true" />
          </Link>
          <Link
            to="/traces"
            className="ms-press inline-flex h-9 items-center rounded-md bg-bg-2 px-3 font-sans text-sm font-strong text-tx-1 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0"
          >
            {t('first_run.explore_traces')}
          </Link>
        </>
      }
      steps={(['instrument', 'send', 'watch'] as const).map((step) => ({
        title: t(`first_run.steps.${step}.title`),
        description: t(`first_run.steps.${step}.description`),
      }))}
    />
  );
}
