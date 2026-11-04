import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { formatRelativeMicros } from '@/time/relative';
import type { AlertRule, Incident } from '@/types/alerting';

import { formatFigure, lineOf, operatorSymbol, SEVERITY_TEXT } from './model';
import { Section } from '../performance/PanelSection';

/** A link set in text, as the drawers of the module set theirs. */
export const TEXT_LINK =
  'shrink-0 border-0 bg-transparent p-0 font-sans text-xs font-strong text-indigo-soft outline-none hover:underline focus-visible:underline';

/**
 * The incidents going on for the application, the worst first: how bad, which rule it is, the line
 * the rule pages on and when it opened. An incident opens in the incident drawer over the page.
 */
export function GoingOn({
  incidents,
  rules,
  pending,
  failed,
  onRetry,
  onOpen,
}: {
  incidents: readonly Incident[];
  rules: readonly AlertRule[];
  pending: boolean;
  failed: boolean;
  onRetry: () => void;
  onOpen: (id: string) => void;
}) {
  const { t, i18n } = useTranslation('rum');
  const { t: ta } = useTranslation('alerts');
  const language = i18n.resolvedLanguage ?? i18n.language;

  return (
    <Section
      title={t('alerts.going_on.title')}
      count={pending || failed ? undefined : incidents.length}
      pending={pending}
      failed={failed}
      onRetry={onRetry}
      empty={incidents.length === 0}
      emptyText={t('alerts.going_on.empty')}
      testId="alerts-going-on"
    >
      <ul data-rum-alert-incidents="" className="m-0 flex list-none flex-col p-0">
        {incidents.map((incident) => {
          const rule = rules.find((candidate) => candidate.id === incident.ruleId);
          const figure = rule?.query.rum;
          const line = rule && figure ? lineOf(rule, incident.severity) : null;
          const detail = [
            figure && ta(`workbench.rum.metrics.${figure.metric}`),
            figure && line && `${operatorSymbol(line.operator)} ${formatFigure(figure.metric, line.threshold, language)}`,
            formatRelativeMicros(incident.createdAt, language),
          ]
            .filter(Boolean)
            .join(' · ');

          return (
            <li
              key={incident.id}
              data-rum-alert-incident={incident.id}
              className="flex items-start gap-3 border-t border-bd-0 py-2.5 text-xs first:border-t-0 first:pt-0"
            >
              <span className={cn('w-14 shrink-0 font-strong', SEVERITY_TEXT[incident.severity])}>{ta(`severity.${incident.severity}`)}</span>
              <span className="min-w-0 flex-1">
                <span className="block truncate font-strong text-tx-0">{incident.summary}</span>
                <span className="block truncate text-tx-3">{detail}</span>
              </span>
              <button type="button" onClick={() => onOpen(incident.id)} className={TEXT_LINK}>
                {t('alerts.going_on.open')}
              </button>
            </li>
          );
        })}
      </ul>
    </Section>
  );
}
