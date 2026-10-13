import { useTranslation } from 'react-i18next';

import { uiLabelClass } from '@/shell/chrome';
import { FormField, FormInput } from '@/shell/FormDrawer';

import { Segmented } from './Segmented';
import { defaultRouting, type Routing, type TransformStep } from '../PipelineGraph/model';

/** A routing step: every event to one stream, or each to the stream a field of it names. */
export function RouteFields({
  step,
  onChange,
}: {
  step: TransformStep;
  onChange: (patch: Partial<TransformStep>) => void;
}) {
  const { t } = useTranslation('pipelines');
  const routing = step.routing ?? defaultRouting;
  const update = (patch: Partial<Routing>) => onChange({ routing: { ...routing, ...patch } });

  return (
    <div className="grid gap-x-[16px] gap-y-[12px] sm:grid-cols-2 xl:grid-cols-3">
      <div className="flex min-w-0 flex-col gap-1.5 sm:col-span-2 xl:col-span-3">
        <span className={uiLabelClass}>{t('realtime.output')}</span>
        <Segmented
          label={t('realtime.output')}
          value={routing.kind}
          onChange={(kind) => update({ kind })}
          className="max-w-[360px]"
          options={[
            { value: 'fixed', label: t('realtime.fixed') },
            { value: 'field', label: t('realtime.field') },
          ]}
        />
      </div>
      {routing.kind === 'fixed' ? (
        <FormField label={t('graph.sink_name')} hint={t('realtime.auto_create')}>
          <FormInput
            value={step.target ?? ''}
            onChange={(event) => onChange({ target: event.target.value })}
            className="font-mono text-xs"
            autoComplete="off"
          />
        </FormField>
      ) : (
        <>
          <FormField label={t('realtime.field_name')} hint={t('realtime.field_hint')}>
            <FormInput
              value={routing.field}
              onChange={(event) => update({ field: event.target.value })}
              placeholder="appname"
              className="font-mono text-xs"
              autoComplete="off"
            />
          </FormField>
          <FormField label={t('realtime.prefix')}>
            <FormInput
              value={routing.prefix}
              onChange={(event) => update({ prefix: event.target.value })}
              placeholder="app_"
              className="font-mono text-xs"
              autoComplete="off"
            />
          </FormField>
          <FormField
            label={t('realtime.fallback')}
            hint={`${t('realtime.fallback_hint')} ${t('realtime.auto_create')}`}
          >
            <FormInput
              value={routing.fallback}
              onChange={(event) => update({ fallback: event.target.value })}
              placeholder="default"
              className="font-mono text-xs"
              autoComplete="off"
            />
          </FormField>
        </>
      )}
    </div>
  );
}
