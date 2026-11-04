import { useTranslation } from 'react-i18next';

import { pipelineChangeText, pipelineTraceOf } from './timelineModel';

/** What an alert pipeline did to the event before the incident opened. */
export function PipelineTraceRow({ detail }: { detail: unknown }) {
  const { t } = useTranslation('alerts');
  const trace = pipelineTraceOf(detail);
  const fieldName = (group: string) =>
    t(`incident_timeline.pipeline_fields.${group}`, { defaultValue: group });
  const changes = trace.changes.map(pipelineChangeText);
  const groups = [...new Set(changes.map((change) => change.group))];
  const said = groups
    .map((group) => {
      const texts = changes.filter((change) => change.group === group).map((change) => change.text);

      return `${fieldName(group)} ${texts.join(t('routing.list_separator'))}`;
    })
    .join(t('routing.list_separator'));

  return (
    <span data-pipeline-trace className="min-w-0 break-words">
      <span className="text-tx-1">{t('incident_timeline.pipeline_processed', { pipeline: trace.pipeline })}</span>
      {said && <span className="text-tx-2">{` ${said}`}</span>}
      {trace.more > 0 && (
        <span className="text-tx-2">{` ${t('incident_timeline.pipeline_more', { count: trace.more })}`}</span>
      )}
      {trace.error && <span className="mt-1 block text-[var(--red-ink)]">{trace.error}</span>}
    </span>
  );
}
