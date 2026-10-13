import { ChevronLeft, ChevronRight, Trash2 } from 'lucide-react';
import type * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ChromeButton, IconButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { surfacePanelClass } from '@/shell/SurfaceWorkbench';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

import { DestinationFields } from './DestinationFields';
import type { PipelineResources } from './resources';
import { SourceFields } from './SourceFields';
import { StepFields } from './StepFields';
import { moveStep, retypeStep } from './steps';
import {
  connectorOf,
  writtenSinkIndex,
  type PipelineGraphModel,
  type StepType,
  type TransformStep,
} from '../PipelineGraph/model';

type NodeKind = 'source' | 'transform' | 'sink';

export function parseNodeId(id: string): { kind: NodeKind; index: number } | null {
  const match = /^(source|transform|sink)-(\d+)$/.exec(id);

  return match ? { kind: match[1] as NodeKind, index: Number(match[2]) } : null;
}

/**
 * The node picked in the flow, opened to be edited: the source, a step or a
 * destination. The source and the stream that is written are what a pipeline
 * is, so they are changed and never removed; steps and connectors come and go.
 */
export function NodeEditor({
  model,
  selectedId,
  resources,
  readOnly = false,
  onChange,
  onSelect,
  className,
}: {
  model: PipelineGraphModel;
  selectedId: string;
  resources: PipelineResources;
  readOnly?: boolean;
  onChange: (next: PipelineGraphModel) => void;
  onSelect: (id: string) => void;
  className?: string;
}) {
  const { t } = useTranslation('pipelines');
  const node = parseNodeId(selectedId);
  const list =
    node?.kind === 'source' ? model.sources : node?.kind === 'sink' ? model.sinks : model.transforms;
  const step = node?.kind === 'transform' ? model.transforms[node.index] : undefined;

  if (!node || node.index >= list.length) {
    return (
      <section data-pipeline-node-editor className={cn('p-[16px]', surfacePanelClass, className)}>
        <p className="m-0 font-sans text-sm text-tx-3">{t('graph.select_node')}</p>
      </section>
    );
  }

  const sink = node.kind === 'sink' ? (model.sinks[node.index] ?? '') : '';
  const connector = node.kind === 'sink' ? connectorOf(sink) : null;
  const title =
    node.kind === 'transform'
      ? t('editor.step_title', { number: node.index + 1 })
      : connector !== null
        ? t('editor.connector')
        : t(`graph.node_kinds.${node.kind}`);
  const name =
    node.kind === 'transform'
      ? step?.name.trim()
      : node.kind === 'source'
        ? model.sources[node.index]?.trim()
        : connector !== null
          ? (resources.connectorNames[connector] ?? connector)
          : sink.trim();
  const removable =
    node.kind === 'transform' ||
    (node.kind === 'source' ? node.index > 0 : connector !== null || node.index !== writtenSinkIndex(model.sinks));

  const remove = () => {
    if (node.kind === 'source') {
      onChange({ ...model, sources: model.sources.filter((_, at) => at !== node.index) });
    } else if (node.kind === 'sink') {
      onChange({ ...model, sinks: model.sinks.filter((_, at) => at !== node.index) });
    } else {
      onChange({ ...model, transforms: model.transforms.filter((_, at) => at !== node.index) });
    }
    // The node before it takes its place under the cursor; with no step left, the source does.
    onSelect(
      node.kind === 'transform' && model.transforms.length === 1
        ? 'source-0'
        : `${node.kind}-${Math.max(0, node.index - 1)}`,
    );
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
          ? retypeStep(item, type, model.transforms.map((other) => other.name))
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
          <span className="shrink-0 text-md font-strong text-tx-0">{title}</span>
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
        {node.kind === 'source' && (
          <SourceFields model={model} index={node.index} resources={resources} onChange={onChange} />
        )}
        {node.kind === 'sink' && (
          <DestinationFields
            model={model}
            index={node.index}
            resources={resources}
            onChange={onChange}
          />
        )}
        {step && (
          <StepFields
            // Another step is another form: nothing typed in one carries into the next.
            key={selectedId}
            step={step}
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
