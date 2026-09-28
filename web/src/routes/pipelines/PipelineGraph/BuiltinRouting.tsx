import { useTranslation } from 'react-i18next';

import { FormField, FormInput } from '@/shell/FormDrawer';

import { defaultRouting, type TransformStep, type Routing } from './model';

export function BuiltinRouting({ step, onChange }: {
  step: TransformStep;
  onChange: (patch: Partial<TransformStep>) => void;
}) {
  const { t } = useTranslation('pipelines');
  const routing = step.routing ?? defaultRouting;
  const update = (patch: Partial<Routing>) => onChange({ routing: { ...routing, ...patch } });
  return <>
    <fieldset className="min-w-0"><legend className="mb-1.5 text-xs text-tx-3">{t('realtime.output')}</legend>
      <div className="flex rounded-md bg-bg-2 p-1" role="group" aria-label={t('realtime.output')}>
        {(['fixed', 'field'] as const).map((kind) => (
          <button key={kind} type="button" aria-pressed={routing.kind === kind} onClick={() => update({ kind })}
            className={`min-h-8 flex-1 rounded px-2 text-xs focus-visible:bg-bg-3 ${routing.kind === kind ? 'bg-indigo-dim text-indigo-soft' : 'text-tx-2 hover:bg-bg-1'}`}>
            {t(`realtime.${kind}`)}
          </button>
        ))}
      </div>
    </fieldset>
    {routing.kind === 'fixed' ? <FormField label={t('graph.sink_name')} hint={t('realtime.auto_create')}>
      <FormInput value={step.target ?? 'default'} onChange={(event) => onChange({ target: event.target.value })} />
    </FormField> : <>
      <FormField label={t('realtime.field_name')} hint={t('realtime.field_hint')}>
        <FormInput value={routing.field} onChange={(event) => update({ field: event.target.value })} placeholder="appname" />
      </FormField>
      <FormField label={t('realtime.prefix')}>
        <FormInput value={routing.prefix} onChange={(event) => update({ prefix: event.target.value })} placeholder="app_" />
      </FormField>
      <FormField label={t('realtime.fallback')} hint={t('realtime.fallback_hint')}>
        <FormInput value={routing.fallback} onChange={(event) => update({ fallback: event.target.value })} placeholder="default" />
      </FormField>
      <p className="text-xs text-tx-3">{t('realtime.auto_create')}</p>
    </>}
  </>;
}
