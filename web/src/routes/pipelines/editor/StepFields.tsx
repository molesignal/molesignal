import { Code2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ChromeButton, uiLabelClass } from '@/shell/chrome';
import { FormField, FormInput } from '@/shell/FormDrawer';
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectSeparator,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';

import { RouteFields } from './RouteFields';
import { VrlFields } from './VrlFields';
import { stepType, type StepType, type TransformStep } from '../PipelineGraph/model';
import { compileProcessor, PROCESSOR_GROUPS } from '../processors/catalog';
import { ProcessorFields } from '../processors/ProcessorFields';

/**
 * One processing step: its name, what kind of processing it is, and that
 * kind's own form. A processor shows the VRL it stands for, and can be turned
 * into VRL to be taken further by hand.
 */
export function StepFields({
  step,
  readOnly = false,
  onChange,
  onRetype,
}: {
  step: TransformStep;
  readOnly?: boolean;
  onChange: (patch: Partial<TransformStep>) => void;
  onRetype: (type: StepType) => void;
}) {
  const { t } = useTranslation('pipelines');
  const type = stepType(step);

  return (
    <div className="grid gap-[14px]">
      <div className="grid gap-x-[16px] gap-y-[12px] sm:grid-cols-2 xl:grid-cols-3">
        <FormField label={t('steps.name')}>
          <FormInput
            value={step.name}
            onChange={(event) => onChange({ name: event.target.value })}
            autoComplete="off"
          />
        </FormField>
        <div className="flex min-w-0 flex-col gap-1.5">
          <span className={uiLabelClass}>{t('processing.type')}</span>
          <Select value={type} onValueChange={(next) => onRetype(next as StepType)}>
            <SelectTrigger aria-label={t('processing.type')} className="h-9 text-sm">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {PROCESSOR_GROUPS.map(({ group, ops }) => (
                <SelectGroup key={group}>
                  <SelectLabel>{t(`steps.groups.${group}`)}</SelectLabel>
                  {ops.map((op) => (
                    <SelectItem key={op} value={op} className="text-xs">
                      {t(`steps.types.${op}`)}
                    </SelectItem>
                  ))}
                </SelectGroup>
              ))}
              <SelectSeparator />
              <SelectItem value="route" className="text-xs">
                {t('steps.types.route')}
              </SelectItem>
              <SelectItem value="vrl" className="text-xs">
                {t('steps.types.vrl')}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>
      </div>

      {step.kind === 'builtin' ? (
        <RouteFields step={step} onChange={onChange} />
      ) : step.processor ? (
        <>
          <ProcessorFields
            config={step.processor}
            onChange={(processor) => onChange({ processor, script: compileProcessor(processor) })}
          />
          <GeneratedScript
            script={step.script}
            // The form is let go of; the script it wrote stays and is now the step.
            onDetach={readOnly ? undefined : () => onRetype('vrl')}
          />
        </>
      ) : (
        <VrlFields
          name={step.name}
          script={step.script}
          readOnly={readOnly}
          onChange={(script) => onChange({ script })}
        />
      )}
    </div>
  );
}

/** The VRL a processor's form amounts to, for those who want to see what will run. */
function GeneratedScript({ script, onDetach }: { script: string; onDetach?: (() => void) | undefined }) {
  const { t } = useTranslation('pipelines');
  const [open, setOpen] = React.useState(false);
  const id = React.useId();

  return (
    <div data-generated-vrl>
      <div className="flex flex-wrap items-center gap-[8px]">
        <ChromeButton
          size="sm"
          variant="ghost"
          aria-expanded={open}
          aria-controls={id}
          onClick={() => setOpen((current) => !current)}
        >
          <Code2 aria-hidden className="h-3.5 w-3.5" />
          {t(open ? 'steps.hide_vrl' : 'steps.show_vrl')}
        </ChromeButton>
        {open && onDetach && (
          <ChromeButton size="sm" variant="ghost" onClick={onDetach}>
            {t('steps.edit_as_vrl')}
          </ChromeButton>
        )}
      </div>
      {open && (
        <pre
          id={id}
          // A long line scrolls sideways; the keyboard reaches it like any other region.
          tabIndex={0}
          aria-label={t('steps.generated_vrl')}
          className="m-0 mt-[8px] overflow-x-auto rounded-md bg-bg-0 px-4 py-3 font-code text-xs leading-[1.7] text-tx-1 transition-colors duration-fast focus-visible:bg-bg-2"
        >
          {script}
        </pre>
      )}
    </div>
  );
}
