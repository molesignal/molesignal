import { ChevronLeft, ChevronRight, Trash2 } from 'lucide-react';
import type * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { ScheduledPipeline } from '@/api/pipelines';
import { ChromeButton, IconButton, uiSectionTitleClass } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { surfacePanelClass } from '@/shell/SurfaceWorkbench';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

import { AlertDestinationNote, AlertSourceFields } from './AlertFields';
import { DestinationFields } from './DestinationFields';
import { canRemoveNode, parseNodeId, removeNode } from './nodes';
import type { PipelineResources } from './resources';
import { InPlaceNote, SourceFields } from './SourceFields';
import { StepFields } from './StepFields';
import { moveStep, retypeStep } from './steps';
import { countingKinds } from '../counting/model';
import {
  changesInPlace,
  connectorOf,
  tableOf,
  type PipelineGraphModel,
  type StepType,
  type TransformStep,
} from '../PipelineGraph/model';

/**
 * The node picked in the flow, opened to be edited: a source, a step or a
 * destination. A pipeline reads at least one stream and writes at least one,
 * so the last of each is changed and never removed; everything else comes
 * and goes.
 */
export function NodeEditor({
  model,
  selectedId,
  resources,
  saved = null,
  readOnly = false,
  onChange,
  onSelect,
  className,
}: {
  model: PipelineGraphModel;
  selectedId: string;
  resources: PipelineResources;
  /** The pipeline as it was last saved; `null` for a new one. */
  saved?: ScheduledPipeline | null;
  readOnly?: boolean;
  onChange: (next: PipelineGraphModel) => void;
  onSelect: (id: string) => void;
  className?: string;
}) {
  const { t } = useTranslation('pipelines');
  const node = parseNodeId(selectedId);
  const alert = model.mode === 'alert';
  const inPlace = changesInPlace(model);
  // An alert pipeline's one source and one destination are not in the lists, and neither is the
  // stream metrics or spans changed as they arrive go back into.
  const count =
    node?.kind === 'transform'
      ? model.transforms.length
      : alert || (inPlace && node?.kind === 'sink')
        ? 1
        : node?.kind === 'source'
          ? model.sources.length
          : model.sinks.length;
  const step = node?.kind === 'transform' ? model.transforms[node.index] : undefined;

  if (!node || node.index >= count) {
    return (
      <section data-pipeline-node-editor className={cn('p-[16px]', surfacePanelClass, className)}>
        <p className="m-0 font-sans text-sm text-tx-3">{t('graph.select_node')}</p>
      </section>
    );
  }

  const writesBack = inPlace && node.kind === 'sink';
  const sink = node.kind === 'sink' && !alert && !writesBack ? (model.sinks[node.index] ?? '') : '';
  const connector = node.kind === 'sink' ? connectorOf(sink) : null;
  const table = node.kind === 'sink' ? tableOf(sink) : null;
  const title =
    node.kind === 'transform'
      ? t('editor.step_title', { number: node.index + 1 })
      : alert
        ? t(
            node.kind === 'source'
              ? 'alert_mode.source_title'
              : model.alert.stage === 'notify'
                ? 'alert_mode.write_back_notify'
                : 'alert_mode.write_back',
          )
        : writesBack
          ? t('realtime.in_place_title')
          : connector !== null
            ? t('editor.connector')
            : table !== null
              ? t('table_sink.table')
              : t(`graph.node_kinds.${node.kind}`);
  const name =
    node.kind === 'transform'
      ? step?.name.trim()
      : alert
        ? ''
        : node.kind === 'source' || writesBack
          ? model.sources[writesBack ? 0 : node.index]?.trim()
          : connector !== null
            ? (resources.connectorNames[connector] ?? connector)
            : (table ?? sink.trim());
  const removable = canRemoveNode(model, selectedId);
  const remove = () => {
    if (readOnly) return;
    const next = removeNode(model, selectedId, selectedId);
    onChange(next.model);
    onSelect(next.selectedId);
  };
  const move = (direction: -1 | 1) => {
    onChange({ ...model, transforms: moveStep(model.transforms, node.index, direction) });
    onSelect(`transform-${node.index + direction}`);
  };
  const setStep = (patch: Partial<TransformStep>) =>
    onChange({
      ...model,
      transforms: model.transforms.map((item, at) => (at === node.index ? { ...item, ...patch } : item)),
    });
  const retype = (type: StepType) =>
    onChange({
      ...model,
      transforms: model.transforms.map((item, at) =>
        at === node.index
          ? retypeStep(item, type, model.transforms.map((other) => other.name), model.mode, model.sources[0])
          : item,
      ),
    });

  return (
    <section
      data-pipeline-node-editor={selectedId}
      aria-label={t('workspace.node_configuration')}
      className={cn('min-w-0 px-[16px] pb-[16px] pt-[12px]', surfacePanelClass, className)}
    >
      <header className="flex min-h-[36px] items-center gap-[10px]">
        <h2 className="m-0 flex min-w-0 items-baseline gap-[8px] font-sans">
          <span className={cn('shrink-0', uiSectionTitleClass)}>{title}</span>
          {name && <span className="min-w-0 truncate font-mono text-xs text-tx-3">{name}</span>}
        </h2>
        <div className="ml-auto flex shrink-0 items-center gap-[2px]">
          {node.kind === 'transform' && model.transforms.length > 1 && (
            <>
              <NodeAction
                label={t('editor.move_earlier')}
                disabled={readOnly || node.index === 0}
                onClick={() => move(-1)}
              >
                <ChevronLeft aria-hidden className="h-4 w-4" />
              </NodeAction>
              <NodeAction
                label={t('editor.move_later')}
                disabled={readOnly || node.index === model.transforms.length - 1}
                onClick={() => move(1)}
              >
                <ChevronRight aria-hidden className="h-4 w-4" />
              </NodeAction>
            </>
          )}
          {removable && (
            <ChromeButton
              size="sm"
              variant="ghost"
              className="text-tx-2 enabled:hover:bg-red-dim enabled:hover:text-red-soft"
              disabled={readOnly}
              onClick={remove}
            >
              <Trash2 aria-hidden className="h-3.5 w-3.5" />
              {t('editor.remove')}
            </ChromeButton>
          )}
        </div>
      </header>

      <fieldset disabled={readOnly} className="m-0 mt-[10px] min-w-0 border-0 p-0">
        {node.kind === 'source' &&
          (alert ? (
            <AlertSourceFields model={model} onChange={onChange} />
          ) : (
            <SourceFields
              model={model}
              index={node.index}
              resources={resources}
              saved={saved}
              canSave={!readOnly}
              onChange={onChange}
            />
          ))}
        {node.kind === 'sink' &&
          (alert ? (
            <AlertDestinationNote stage={model.alert.stage} />
          ) : writesBack ? (
            <InPlaceNote signal={model.signalType} source={model.sources[0]?.trim() ?? ''} />
          ) : (
            <DestinationFields
              model={model}
              index={node.index}
              resources={resources}
              onChange={onChange}
            />
          ))}
        {step && (
          <StepFields
            // Another step is another form: nothing typed in one carries into the next.
            key={selectedId}
            step={step}
            mode={model.mode}
            stage={model.alert.stage}
            resources={resources}
            routes={!inPlace}
            counting={countingKinds(model.mode, model.signalType, model.sources[0] ?? '')}
            source={model.sources[0]?.trim() ?? ''}
            tables={resources.tables}
            readOnly={readOnly}
            onChange={setStep}
            onRetype={retype}
          />
        )}
      </fieldset>
    </section>
  );
}

function NodeAction({
  label,
  disabled,
  onClick,
  children,
}: {
  label: string;
  disabled: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span className="inline-flex">
          <IconButton aria-label={label} disabled={disabled} onClick={onClick}>
            {children}
          </IconButton>
        </span>
      </TooltipTrigger>
      <TooltipContent side="top">{label}</TooltipContent>
    </Tooltip>
  );
}
