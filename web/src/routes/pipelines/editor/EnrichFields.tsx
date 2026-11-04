import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { uiLabelClass } from '@/shell/chrome';
import { FormField, FormInput, FormSelect, FormTextarea } from '@/shell/FormDrawer';

import type { PipelineResources } from './resources';
import { Segmented } from './Segmented';
import { StreamField } from './StreamField';
import {
  DEFAULT_QUERY_ANNOTATION,
  defaultEnrichQuery,
  MAX_QUERY_ROWS,
  MAX_QUERY_WINDOW_SECS,
  type EnrichQuerySettings,
  type TransformStep,
} from '../PipelineGraph/model';

// What goes in a field is code, not copy: the examples read the same in every language.
const EXAMPLE = {
  sql: "SELECT count(*) AS errors FROM app_logs WHERE level = 'error' AND host = '{{labels.host}}'",
  promql: 'sum(rate(http_requests_total{instance="{{labels.host}}",status=~"5.."}[5m]))',
  placeholders: '{{labels.host}}, {{annotations.x}}, {{summary}}, {{severity}}, {{rule_name}}, {{source}}',
} as const;

/**
 * Calling an API: the alert event is posted to an HTTP connector, whose address
 * and credential are the connector's, and what the answer says of the event's
 * fields is taken.
 */
export function HttpEnrichFields({
  step,
  resources,
  onChange,
}: {
  step: TransformStep;
  resources: PipelineResources | undefined;
  onChange: (patch: Partial<TransformStep>) => void;
}) {
  const { t } = useTranslation('pipelines');
  const connectors = (resources?.connectors ?? []).filter((connector) => connector.kind === 'http');

  return (
    <div className="grid max-w-[640px] gap-[12px]">
      <FormField label={t('alert_mode.http.connector')} hint={t('alert_mode.http.connector_hint')}>
        <FormSelect
          value={step.connectorId ?? ''}
          onChange={(connectorId) => onChange({ connectorId })}
          ariaLabel={t('alert_mode.http.connector')}
          placeholder={t('alert_mode.http.connector_placeholder')}
          options={connectors.map((connector) => ({ value: connector.id, label: connector.name }))}
        />
      </FormField>
      {connectors.length === 0 && (
        <p className="m-0 font-sans text-xs leading-relaxed text-tx-3">
          {t('alert_mode.http.none')}{' '}
          <Link to="/pipelines/connectors" className="text-tx-1 underline-offset-2 hover:underline">
            {t('alert_mode.http.manage')}
          </Link>
        </p>
      )}
      <p className="m-0 font-sans text-sm leading-relaxed text-tx-2">{t('alert_mode.http.note')}</p>
    </div>
  );
}

/**
 * A query enrich step: a statement with values of the event in it, how far back
 * it looks, and the annotation its rows go into.
 */
export function QueryEnrichFields({
  step,
  resources,
  onChange,
}: {
  step: TransformStep;
  resources: PipelineResources | undefined;
  onChange: (patch: Partial<TransformStep>) => void;
}) {
  const { t } = useTranslation('pipelines');
  const query = step.query ?? defaultEnrichQuery;
  const set = (patch: Partial<EnrichQuerySettings>) => onChange({ query: { ...query, ...patch } });

  return (
    <div className="grid gap-[12px]">
      <div className="grid gap-x-[16px] gap-y-[12px] sm:grid-cols-2 xl:grid-cols-3">
        <div className="flex min-w-0 flex-col gap-1.5">
          <span className={uiLabelClass}>{t('alert_mode.query.language')}</span>
          <Segmented
            label={t('alert_mode.query.language')}
            value={query.language}
            // PromQL reads metrics; SQL keeps the kind of stream it had.
            onChange={(language) =>
              set(language === 'promql' ? { language, streamType: 'metrics', stream: '' } : { language })
            }
            options={[
              { value: 'sql', label: 'SQL' },
              { value: 'promql', label: 'PromQL' },
            ]}
          />
        </div>
        {query.language === 'sql' && (
          <FormField label={t('alert_mode.query.stream_type')}>
            <FormSelect
              value={query.streamType}
              onChange={(streamType) =>
                set({ streamType: streamType as EnrichQuerySettings['streamType'], stream: '' })
              }
              ariaLabel={t('alert_mode.query.stream_type')}
              options={[
                { value: 'logs', label: t('filters.logs') },
                { value: 'metrics', label: t('filters.metrics') },
                { value: 'traces', label: t('filters.traces') },
              ]}
            />
          </FormField>
        )}
        {resources && (
          <StreamField
            label={t('alert_mode.query.stream')}
            hint={t('alert_mode.query.stream_hint')}
            value={query.stream}
            signalType={query.streamType}
            resources={resources}
            creatable={false}
            onChange={(stream) => set({ stream })}
          />
        )}
      </div>

      <FormField
        label={t('alert_mode.query.statement')}
        hint={t('alert_mode.query.statement_hint', { placeholders: EXAMPLE.placeholders })}
      >
        <FormTextarea
          value={query.statement}
          onChange={(event) => set({ statement: event.target.value })}
          placeholder={EXAMPLE[query.language]}
          rows={4}
          className="font-mono text-xs"
          spellCheck={false}
        />
      </FormField>

      <div className="grid gap-x-[16px] gap-y-[12px] sm:grid-cols-2 xl:grid-cols-3">
        <FormField label={t('alert_mode.query.window')} hint={t('alert_mode.query.window_hint')}>
          <NumberInput
            value={query.windowSecs}
            max={MAX_QUERY_WINDOW_SECS}
            onChange={(windowSecs) => set({ windowSecs })}
          />
        </FormField>
        <FormField
          label={t('alert_mode.query.max_rows')}
          hint={t('alert_mode.query.max_rows_hint', { max: MAX_QUERY_ROWS })}
        >
          <NumberInput value={query.maxRows} max={MAX_QUERY_ROWS} onChange={(maxRows) => set({ maxRows })} />
        </FormField>
        <FormField label={t('alert_mode.annotation')} hint={t('alert_mode.query.annotation_hint')}>
          <FormInput
            value={query.annotation}
            onChange={(event) => set({ annotation: event.target.value })}
            placeholder={DEFAULT_QUERY_ANNOTATION}
            className="font-mono text-xs"
            autoComplete="off"
            spellCheck={false}
          />
        </FormField>
      </div>
    </div>
  );
}

/** A whole number from 1 to `max`. */
function NumberInput({
  value,
  max,
  onChange,
}: {
  value: number;
  max: number;
  onChange: (value: number) => void;
}) {
  return (
    <FormInput
      type="number"
      inputMode="numeric"
      min={1}
      max={max}
      value={String(value)}
      onChange={(event) => {
        const next = Number.parseInt(event.target.value, 10);
        if (Number.isFinite(next)) onChange(Math.min(Math.max(next, 1), max));
      }}
      className="font-mono text-xs"
    />
  );
}
