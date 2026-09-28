import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';

import { FormField, FormInput } from '@/shell/FormDrawer';

import { BuiltinRouting } from './BuiltinRouting';
import { defaultRouting, type TransformStep } from './model';

/** Each processing node is either a built-in operation or a VRL function. */
export function ProcessingEditor({ step, onChange, vrlEditor }: {
  step: TransformStep;
  onChange: (patch: Partial<TransformStep>) => void;
  vrlEditor: ReactNode;
}) {
  const { t } = useTranslation('pipelines');
  return <>
    <FormField label={t('graph.transform_name')}>
      <FormInput value={step.name} onChange={(event) => onChange({ name: event.target.value })} />
    </FormField>
    <fieldset className="min-w-0">
      <legend className="mb-1.5 text-xs text-tx-3">{t('processing.type')}</legend>
      <div className="flex rounded-md bg-bg-2 p-1" role="group" aria-label={t('processing.type')}>
        {(['builtin', 'vrl'] as const).map((kind) => <button key={kind} type="button"
          aria-pressed={(step.kind ?? 'vrl') === kind}
          className={`min-h-8 flex-1 rounded px-2 text-xs focus-visible:bg-bg-3 ${(step.kind ?? 'vrl') === kind ? 'bg-indigo-dim text-indigo-soft' : 'text-tx-2 hover:bg-bg-1'}`}
          onClick={() => onChange({ kind, ...(kind === 'builtin' ? { operation: 'route', routing: step.routing ?? { ...defaultRouting }, target: step.target ?? 'default' } : { script: step.script || '. = .' }) })}>
          {t(`processing.${kind}`)}
        </button>)}
      </div>
    </fieldset>
    {step.kind === 'builtin' ? <>
      <div className="text-xs font-medium text-tx-1">{t('processing.route')}</div>
      <BuiltinRouting step={step} onChange={onChange} />
    </> : vrlEditor}
  </>;
}
