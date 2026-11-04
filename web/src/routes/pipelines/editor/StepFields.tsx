import { Code2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { ExtendTableSummary } from '@/api/extendTables';
import { ChromeButton, uiLabelClass } from '@/shell/chrome';
import { FormField, FormInput, FormTextarea } from '@/shell/FormDrawer';
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

import { HttpEnrichFields, QueryEnrichFields } from './EnrichFields';
import type { PipelineResources } from './resources';
import { RouteFields } from './RouteFields';
import { VrlFields } from './VrlFields';
import { CountingFields } from '../counting/CountingFields';
import { isCountingKind, type CountingKind } from '../counting/model';
import {
  DEFAULT_SUMMARY_ANNOTATION,
  stepType,
  type AlertStage,
  type PipelineMode,
  type StepType,
  type TransformStep,
} from '../PipelineGraph/model';
import { compileProcessor, PROCESSOR_GROUPS, type ProcessorOp } from '../processors/catalog';
import { ProcessorFields } from '../processors/ProcessorFields';

/** What sampling and routing mean is a stream of events: an alert pipeline offers neither. */
const NOT_FOR_ALERTS: readonly ProcessorOp[] = ['sample'];

/** The steps of an alert pipeline that go outside MoleSignal, as an incident opens. */
export const OUTSIDE_STEPS = ['ai_summary', 'http_enrich', 'query_enrich'] as const;

/** The processors a pipeline of this mode offers, by group. */
export function processorGroups(mode: PipelineMode | undefined) {
  return mode === 'alert'
    ? PROCESSOR_GROUPS.map(({ group, ops }) => ({ group, ops: ops.filter((op) => !NOT_FOR_ALERTS.includes(op)) }))
    : PROCESSOR_GROUPS;
}

/**
 * One processing step: its name, what kind of processing it is, and that
 * kind's own form. A processor shows the VRL it stands for, and can be turned
 * into VRL to be taken further by hand.
 */
export function StepFields({
  step,
  mode,
  stage = 'open',
  resources,
  routes = true,
  counting = [],
  source = '',
  tables = [],
  readOnly = false,
  onChange,
  onRetype,
}: {
  step: TransformStep;
  /** The pipeline's mode: an alert pipeline has AI summaries and no routing. */
  mode?: PipelineMode | undefined;
  /** When an alert pipeline runs: only one that runs as an incident opens has steps that go outside. */
  stage?: AlertStage | undefined;
  /** The connectors and streams a step that goes outside can use. */
  resources?: PipelineResources | undefined;
  /** Whether a step may route events to other streams: not where they stay in their stream. */
  routes?: boolean;
  /** The steps that keep count this pipeline can take (`countingKinds`). */
  counting?: readonly CountingKind[];
  /** The stream the pipeline reads. */
  source?: string;
  /** The extend tables a lookup can read. */
  tables?: readonly ExtendTableSummary[];
  readOnly?: boolean;
  onChange: (patch: Partial<TransformStep>) => void;
  onRetype: (type: StepType) => void;
}) {
  const { t } = useTranslation('pipelines');
  const type = stepType(step);
  const alert = mode === 'alert';

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
              {processorGroups(mode).map(({ group, ops }) => (
                <SelectGroup key={group}>
                  <SelectLabel>{t(`steps.groups.${group}`)}</SelectLabel>
                  {ops.map((op) => (
                    <SelectItem key={op} value={op} className="text-xs">
                      {t(`steps.types.${op}`)}
                    </SelectItem>
                  ))}
                </SelectGroup>
              ))}
              {(counting.length > 0 || isCountingKind(type)) && (
                <SelectGroup>
                  <SelectLabel>{t('steps.groups.counting')}</SelectLabel>
                  {(counting.includes(type as CountingKind) || !isCountingKind(type) ? counting : [...counting, type]).map(
                    (kind) => (
                      <SelectItem key={kind} value={kind} className="text-xs">
                        {t(`steps.types.${kind}`)}
                      </SelectItem>
                    ),
                  )}
                </SelectGroup>
              )}
              <SelectSeparator />
              {alert ? (
                (stage === 'open' || type === 'ai_summary' || type === 'http_enrich' || type === 'query_enrich') &&
                OUTSIDE_STEPS.map((outside) => (
                  <SelectItem key={outside} value={outside} className="text-xs">
                    {t(`steps.types.${outside}`)}
                  </SelectItem>
                ))
              ) : (
                (routes || type === 'route') && (
                  <SelectItem value="route" className="text-xs">
                    {t('steps.types.route')}
                  </SelectItem>
                )
              )}
              <SelectItem value="vrl" className="text-xs">
                {t('steps.types.vrl')}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>
      </div>

      {isCountingKind(step.kind) ? (
        <CountingFields
          kind={step.kind}
          step={step}
          source={source}
          routes={routes}
          resources={resources}
          onChange={onChange}
        />
      ) : step.kind === 'builtin' ? (
        <RouteFields step={step} onChange={onChange} />
      ) : step.kind === 'ai_summary' ? (
        <SummaryFields step={step} onChange={onChange} />
      ) : step.kind === 'http_enrich' ? (
        <HttpEnrichFields step={step} resources={resources} onChange={onChange} />
      ) : step.kind === 'query_enrich' ? (
        <QueryEnrichFields step={step} resources={resources} onChange={onChange} />
      ) : step.processor ? (
        <>
          <ProcessorFields
            config={step.processor}
            tables={tables}
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

/**
 * An AI summary: what the organization's model is asked about the alert event,
 * and the annotation its answer goes into.
 */
function SummaryFields({
  step,
  onChange,
}: {
  step: TransformStep;
  onChange: (patch: Partial<TransformStep>) => void;
}) {
  const { t } = useTranslation('pipelines');

  return (
    <div className="grid gap-[12px]">
      <FormField label={t('alert_mode.prompt')} hint={t('alert_mode.prompt_hint')}>
        <FormTextarea
          value={step.prompt ?? ''}
          onChange={(event) => onChange({ prompt: event.target.value })}
          rows={4}
        />
      </FormField>
      <FormField label={t('alert_mode.annotation')} hint={t('alert_mode.annotation_hint')} className="max-w-[320px]">
        <FormInput
          value={step.annotation ?? ''}
          onChange={(event) => onChange({ annotation: event.target.value })}
          placeholder={DEFAULT_SUMMARY_ANNOTATION}
          className="font-mono text-xs"
          autoComplete="off"
          spellCheck={false}
        />
      </FormField>
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
