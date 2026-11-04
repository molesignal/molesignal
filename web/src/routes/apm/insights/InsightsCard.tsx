import { Lightbulb } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { Insight, InsightSeverity, InsightsResponse } from '@/api/apmInsights';
import { useTimeFormatter } from '@/lib/time';
import { cn } from '@/shell/lib/cn';
import { formatRelativeMicros } from '@/time/relative';

import { InvestigateButton } from '../frame/investigate';
import { ApmCard } from '../frame/Page';
import { servicePath } from '../scope';
import { BaselineTable } from './BaselineTable';
import { changeWords, insightSentence } from './sentence';

const SEVERITY_TEXT: Record<InsightSeverity, string> = {
  critical: 'text-[var(--red-ink)]',
  warning: 'text-[var(--yellow-ink)]',
  info: 'text-[var(--blue-ink)]',
};

/**
 * What APM finds worth a look in the range, the most pressing first: moves
 * against the stretch before and the same stretch a week before with the
 * changes near when they began, versions that did worse, and where time
 * could be won back. Each finding can be handed to Mole Agent. Over them,
 * the stretch before and the same stretch a week before against now.
 */
export function InsightsCard({
  data,
  range,
  service,
  hide = [],
}: {
  data: InsightsResponse;
  range: { from: Date; to: Date };
  /** The service a service's page is about; none on the overview. */
  service?: { name: string; environment: string } | undefined;
  /** Kinds another card on the page already says. */
  hide?: readonly Insight['kind'][];
}) {
  const { t } = useTranslation('apm');
  const insights = data.insights.filter((insight) => !hide.includes(insight.kind));

  return (
    <ApmCard icon={Lightbulb} tone="text-[var(--yellow-ink)]" title={t('insights.title')} testId="insights">
      <BaselineTable data={data} />
      {insights.length === 0 ? (
        <p className="border-t border-bd-0 px-3 py-4 font-sans text-xs text-tx-3">{t('insights.none')}</p>
      ) : (
        // Rows share their columns, so the labels line up whatever their language.
        <ul className="grid grid-cols-[max-content_minmax(0,1fr)_max-content] gap-x-3 divide-y divide-bd-0 border-t border-bd-0">
          {insights.map((insight, index) => (
            <InsightRow key={`${insight.kind}:${insight.service.name}:${insight.service.environment}:${index}`} insight={insight} range={range} inService={Boolean(service)} />
          ))}
        </ul>
      )}
    </ApmCard>
  );
}

function InsightRow({ insight, range, inService }: { insight: Insight; range: { from: Date; to: Date }; inService: boolean }) {
  const { t, i18n } = useTranslation('apm');
  const time = useTimeFormatter();
  const sentence = insightSentence(t, insight);
  const changes = insight.changes ?? [];

  return (
    <li data-apm-insight={insight.kind} data-severity={insight.severity} className="col-span-3 grid grid-cols-subgrid items-start px-3 py-2 font-sans text-xs">
      <span className="flex flex-col">
        <span className={cn('font-strong', SEVERITY_TEXT[insight.severity])}>{t(`insights.severity.${insight.severity}`)}</span>
        <span className="text-tx-2">{t(`insights.kind.${insight.kind}`)}</span>
      </span>
      <span className="min-w-0 space-y-0.5">
        <span className="block text-tx-1">
          {/* A link inside a sentence is underlined, faintly: its weight alone does not tell it from the words round it on dark. */}
          {!inService && (
            <Link
              to={servicePath(insight.service)}
              className="mr-1 font-strong text-tx-0 underline decoration-[var(--bd-2)] underline-offset-[3px] hover:decoration-current focus-visible:decoration-current focus-visible:outline-none"
            >
              {insight.service.name}
            </Link>
          )}
          {sentence}
          {insight.since !== undefined && (
            <span className="text-tx-3" title={time.micros(insight.since, false)}>
              {' · '}
              {t('insights.since', { at: formatRelativeMicros(insight.since, i18n.language) })}
            </span>
          )}
        </span>
        {changes.length > 0 && (
          <span data-apm-insight-changes="" className="block text-tx-2">
            {t('insights.changes_near')}{' '}
            {changes.map((change, at) => (
              <span key={`${change.service}:${change.version}:${change.at}`} className={change.downstream ? 'text-tx-2' : 'text-tx-1'}>
                {at > 0 && t('insights.separator')}
                {changeWords(t, change)}{' '}
                <span className="text-tx-3" title={time.micros(change.at, false)}>
                  {formatRelativeMicros(change.at, i18n.language)}
                </span>
              </span>
            ))}
          </span>
        )}
      </span>
      <InvestigateButton
        compact
        subject={{
          service: { name: insight.service.name, environment: insight.service.environment },
          range,
          endpoint: insight.transaction?.name ?? null,
          finding: sentence,
        }}
      />
    </li>
  );
}
