import { useTranslation } from 'react-i18next';

import { Readout } from '@/shell/readout/Readout';
import { SpectrumBar } from '@/shell/readout/SpectrumBar';
import { STATEMENT_CELL, Statement } from '@/shell/readout/Statement';
import { formatRelativeMicros } from '@/time/relative';
import type { Incident, Severity } from '@/types/alerting';

import { formatDurationSecs } from '../alertRuleModel';
import {
  SEVERITIES_WORST_FIRST,
  SEVERITY_COLOR,
  affectedServices,
  incidentDurationSecs,
  severityCounts,
} from './model';

const SEVERITY_TEXT: Record<Severity, string> = {
  critical: 'text-red',
  error: 'text-orange',
  warning: 'text-yellow',
  info: 'text-blue',
};

/**
 * What the Incidents tab opens with: is anything firing, how bad, and is
 * anyone on it.
 *
 * The verdict takes the colour of the worst incident that is firing, with the
 * spread of severities under it. Beside it are the three things a responder
 * asks next: what nobody has picked up, what is affected, and how the last day
 * went. The list under the statement is what these figures count.
 */
export function IncidentStanding({
  active,
  waiting,
  recovered,
  ruleCount,
  nowMicros,
}: {
  /** Incidents that are firing, acknowledged or not. */
  active: readonly Incident[];
  /** The ones among them nobody has acknowledged. */
  waiting: readonly Incident[];
  /** Incidents that recovered in the last day, the latest first. */
  recovered: readonly Incident[];
  ruleCount: number;
  nowMicros: number;
}) {
  const { t, i18n } = useTranslation('alerts');
  const counts = severityCounts(active);
  const worst = SEVERITIES_WORST_FIRST.find((severity) => counts[severity] > 0);
  const services = affectedServices(active);
  const oldest = active.reduce<Incident | undefined>(
    (first, incident) => (!first || incident.created_at < first.created_at ? incident : first),
    undefined,
  );
  const lastRecovery = recovered[0]?.resolved_at;

  return (
    <Statement
      label={t('center.incidents.standing.label')}
      tone={worst ? SEVERITY_COLOR[worst] : 'var(--green)'}
      eyebrow={t('center.incidents.standing.eyebrow')}
      title={
        worst
          ? t('center.incidents.standing.firing', { count: active.length })
          : t('center.incidents.healthy.title')
      }
      titleClassName={worst ? SEVERITY_TEXT[worst] : 'text-green'}
      detail={
        oldest
          ? t('center.incidents.standing.oldest', {
              duration: formatDurationSecs(incidentDurationSecs(oldest, nowMicros)),
            })
          : t('center.incidents.standing.watching', { count: ruleCount })
      }
      dataAttributes={{ 'data-incident-standing': worst ?? 'quiet' }}
      readouts={
        <>
          <Readout
            className={STATEMENT_CELL}
            riseIndex={1}
            label={t('center.incidents.kpis.waiting')}
            value={String(waiting.length)}
            tone={waiting.length > 0 ? 'warn' : undefined}
            detail={
              waiting.length > 0
                ? t('center.incidents.kpis.waiting_sub')
                : active.length > 0
                  ? t('center.incidents.standing.all_owned')
                  : undefined
            }
          />
          <Readout
            className={STATEMENT_CELL}
            riseIndex={2}
            label={t('center.incidents.kpis.services')}
            value={String(services.length)}
            // Which ones, not only how many: the names are what a reader acts on.
            detail={services.length > 0 ? services.join(', ') : undefined}
          />
          <Readout
            className={STATEMENT_CELL}
            riseIndex={3}
            label={t('center.incidents.kpis.recovered')}
            value={String(recovered.length)}
            detail={
              lastRecovery
                ? t('center.incidents.standing.last_recovery', {
                    when: formatRelativeMicros(lastRecovery, i18n.language, nowMicros),
                  })
                : t('center.incidents.kpis.no_recovery')
            }
          />
        </>
      }
    >
      {worst && (
        <SpectrumBar
          className="mt-[14px]"
          label={t('center.incidents.columns.severity')}
          segments={SEVERITIES_WORST_FIRST.map((severity) => ({
            key: severity,
            label: t(`severity.${severity}`),
            value: counts[severity],
            color: SEVERITY_COLOR[severity],
          }))}
        />
      )}
    </Statement>
  );
}
