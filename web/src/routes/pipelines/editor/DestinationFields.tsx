import { ExternalLink } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { ChromeButton } from '@/shell/chrome';
import { FormField, FormSelect } from '@/shell/FormDrawer';

import type { PipelineResources } from './resources';
import { StreamField } from './StreamField';
import {
  CONNECTOR_PREFIX,
  connectorOf,
  firstStreamIndex,
  isRouted,
  TABLE_PREFIX,
  tableOf,
  type PipelineGraphModel,
} from '../PipelineGraph/model';
import { isWritableStream } from '../PipelineGraph/streamName';

/**
 * Where a pipeline writes: a stream that takes every event, a connector that
 * also carries them out of the workspace, or an extend table that takes a row
 * per event. With a routing step, the first stream stands for the stream the
 * route picks.
 */
export function DestinationFields({
  model,
  index,
  resources,
  onChange,
}: {
  model: PipelineGraphModel;
  index: number;
  resources: PipelineResources;
  onChange: (next: PipelineGraphModel) => void;
}) {
  const { t } = useTranslation('pipelines');
  const value = model.sinks[index] ?? '';
  const connector = connectorOf(value);
  const table = tableOf(value);
  const routed = isRouted(model);
  const setSink = (next: string) =>
    onChange({
      ...model,
      sinks: model.sinks.map((item, at) => (at === index ? next : item)),
    });
  const note = (text: string) => (
    <p className="m-0 max-w-[640px] font-sans text-sm leading-relaxed text-tx-2">{text}</p>
  );

  if (table !== null) {
    const known = resources.tables.find((item) => item.tableName === table);
    const names = resources.tables.map((item) => item.tableName);

    return (
      <div className="flex max-w-[640px] flex-wrap items-end gap-[8px]">
        <FormField
          label={t('table_sink.table')}
          hint={
            known
              ? t('table_sink.hint', { key: known.keyField })
              : t(resources.tables.length > 0 ? 'table_sink.choose' : 'table_sink.none')
          }
          className="min-w-[240px] max-w-[420px] flex-1"
        >
          <FormSelect
            value={table}
            onChange={(name) => setSink(`${TABLE_PREFIX}${name}`)}
            options={table && !names.includes(table) ? [table, ...names] : names}
            placeholder={t('table_sink.placeholder')}
            ariaLabel={t('table_sink.table')}
          />
        </FormField>
        <ChromeButton
          // Extend tables are made on their own page; this pipeline stays open here.
          onClick={() => window.open('/extend-tables', '_blank', 'noopener')}
          className="mb-[26px]"
        >
          <ExternalLink aria-hidden className="h-3.5 w-3.5" />
          {t('table_sink.manage')}
        </ChromeButton>
      </div>
    );
  }

  if (connector !== null) {
    // A connector that is turned off is skipped by every run until it is turned on again.
    const off = resources.connectors.some((item) => item.id === connector && item.enabled === false);
    const hint = off
      ? 'editor.connector_off'
      : resources.connectors.length > 0
        ? 'editor.connector_hint'
        : 'editor.connector_none';

    return (
      <div className="flex max-w-[640px] flex-wrap items-end gap-[8px]">
        <FormField label={t('editor.connector')} hint={t(hint)} className="min-w-[240px] max-w-[420px] flex-1">
          <FormSelect
            value={connector}
            onChange={(id) => setSink(`${CONNECTOR_PREFIX}${id}`)}
            options={resources.connectors.map((item) => ({
              value: item.id,
              label:
                item.enabled === false
                  ? `${item.name} (${item.kind}) · ${t('connectors.state.off')}`
                  : `${item.name} (${item.kind})`,
            }))}
            placeholder={t('editor.connector_placeholder')}
          />
        </FormField>
        <ChromeButton
          // Connectors are made on their own page; this pipeline stays open here.
          onClick={() => window.open('/pipelines/connectors', '_blank', 'noopener')}
          // Level with the field, not with the hint under it.
          className="mb-[26px]"
        >
          <ExternalLink aria-hidden className="h-3.5 w-3.5" />
          {t('editor.manage_connectors')}
        </ChromeButton>
      </div>
    );
  }

  // A routing step picks the stream for every event; there is nothing to choose here.
  if (routed && index === firstStreamIndex(model.sinks)) return note(t('processing.output_hint'));

  return (
    <div className="max-w-[420px]">
      <StreamField
        label={t('graph.sink_name')}
        hint={t(routed ? 'editor.copy_hint' : 'realtime.auto_create')}
        value={value}
        signalType={model.signalType}
        resources={resources}
        creatable
        offers={isWritableStream}
        onChange={setSink}
      />
    </div>
  );
}
