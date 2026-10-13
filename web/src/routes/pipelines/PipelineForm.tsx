import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { PipelineInput, ScheduledPipeline } from '@/api/pipelines';
import { toApiError } from '@/lib/http';
import { cn } from '@/shell/lib/cn';
import { surfacePanelClass } from '@/shell/SurfaceWorkbench';

import { PipelineFlow } from './editor/Flow';
import { FlowIssues, FlowStatus } from './editor/Issues';
import { NodeEditor, parseNodeId } from './editor/NodeEditor';
import { usePipelineResources } from './editor/resources';
import {
  formatInterval,
  intervalFromSeconds,
  intervalSeconds,
  parseInterval,
} from './editor/schedule';
import {
  PipelineSettings,
  type PipelineMode,
  type PipelineSettingsValue,
} from './editor/Settings';
import { newStep } from './editor/steps';
import { TestPanel } from './editor/test/TestPanel';
import {
  blocksSave,
  CONNECTOR_PREFIX,
  connectorOf,
  pipelineGraphFromPipeline,
  pipelineInputFromGraph,
  validateGraph,
  type PipelineGraphModel,
  type PipelineSignalType,
  type StepType,
} from './PipelineGraph';

/**
 * What to tell someone whose save was turned down. The backend compiles every
 * step and names none of them when one fails; the test run does.
 */
export function saveFailureMessage(error: unknown, t: (key: string) => string): string {
  const { message } = toApiError(error);

  return message.includes('invalid pipeline VRL script') ? t('editor.save_vrl_invalid') : message;
}

interface PipelineFormProps {
  formId: string;
  initial?: ScheduledPipeline | null;
  disabled?: boolean;
  disabledReason?: string | undefined;
  enabledDisabled?: boolean;
  enabledDisabledReason?: string | undefined;
  onSubmit: (payload: PipelineInput) => void;
}

const SOURCE = 'source-0';

/** Where an editor opens: on the first step of a pipeline that has one, else on its source. */
function firstNode(graph: PipelineGraphModel): string {
  return graph.transforms.length > 0 ? 'transform-0' : SOURCE;
}

/**
 * The pipeline editor shared by /pipelines/new and /pipelines/:id/edit: what
 * the pipeline is and when it runs, the flow from the source through the
 * steps to the destinations, the node picked in it, and a test run beside that.
 *
 * Persists exactly the fields exposed by `bin/molesignal/src/api/http/routes/
 * scheduled_pipelines.rs::CreateReq`. A flow with errors is not submitted:
 * the backend would reject it, or the save would have to make something up.
 */
export function PipelineForm({
  formId,
  initial,
  disabled = false,
  disabledReason,
  enabledDisabled = false,
  enabledDisabledReason,
  onSubmit,
}: PipelineFormProps) {
  const { t } = useTranslation('pipelines');
  const resources = usePipelineResources();
  const [name, setName] = React.useState(initial?.name ?? '');
  const [every, setEvery] = React.useState(() => parseInterval(initial?.cron));
  const [lookback, setLookback] = React.useState(() => intervalFromSeconds(initial?.lookback_secs));
  const [enabled, setEnabled] = React.useState(initial?.enabled ?? true);
  const [graph, setGraph] = React.useState<PipelineGraphModel>(() =>
    pipelineGraphFromPipeline(initial, 'logs'),
  );
  const [selectedId, setSelectedId] = React.useState(() => firstNode(graph));
  const [heldBack, setHeldBack] = React.useState(false);
  const issuesRef = React.useRef<HTMLDivElement>(null);

  const issues = React.useMemo(
    () => validateGraph(graph, resources.connectorIds),
    [graph, resources.connectorIds],
  );
  const blocked = blocksSave(issues);

  // Once the errors are gone there is nothing left to insist on.
  React.useEffect(() => {
    if (!blocked) setHeldBack(false);
  }, [blocked]);

  // A node that was removed, or a mode that dropped it, leaves the source picked.
  React.useEffect(() => {
    const node = parseNodeId(selectedId);
    const count =
      node?.kind === 'source'
        ? graph.sources.length
        : node?.kind === 'sink'
          ? graph.sinks.length
          : graph.transforms.length;
    if (!node || node.index >= count) setSelectedId(SOURCE);
  }, [graph.sinks.length, graph.sources.length, graph.transforms.length, selectedId]);

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (disabled) return;

    if (blocked) {
      setHeldBack(true);
      issuesRef.current?.scrollIntoView?.({ block: 'nearest' });
      return;
    }

    onSubmit(
      pipelineInputFromGraph({
        name,
        cron: formatInterval(every),
        lookbackSecs: intervalSeconds(lookback),
        enabled,
        graph,
      }),
    );
  };

  const changeMode = (mode: PipelineMode) => {
    setGraph((current) => ({
      ...current,
      mode,
      // Realtime works on logs as they arrive: one stream in, one stream out, no connectors.
      ...(mode === 'realtime' && {
        signalType: 'logs',
        sources: [current.sources[0] ?? ''],
        sinks: [current.sinks.find((sink) => connectorOf(sink) === null) ?? ''],
        retainSource: current.retainSource ?? false,
      }),
    }));
  };

  // Streams belong to a signal: another signal means another source and destination to choose.
  const changeSignalType = (signalType: PipelineSignalType) => {
    setGraph((current) => ({
      ...current,
      signalType,
      sources: [''],
      sinks: ['', ...current.sinks.filter((sink) => connectorOf(sink) !== null)],
    }));
  };

  const changeSettings = (patch: Partial<PipelineSettingsValue>) => {
    if (patch.name !== undefined) setName(patch.name);
    if (patch.every !== undefined) setEvery(patch.every);
    if (patch.lookback !== undefined) setLookback(patch.lookback);
    if (patch.enabled !== undefined) setEnabled(patch.enabled);
  };

  // A new connector starts unchosen: which one it is has to be said, not guessed.
  const addConnector = () => {
    setSelectedId(`sink-${graph.sinks.length}`);
    setGraph({ ...graph, sinks: [...graph.sinks, CONNECTOR_PREFIX] });
  };
  const addStep = (type: StepType) => {
    const step = newStep(type, graph.transforms.map((item) => item.name));
    setSelectedId(`transform-${graph.transforms.length}`);
    setGraph({ ...graph, transforms: [...graph.transforms, step] });
  };

  return (
    <form
      id={formId}
      onSubmit={submit}
      aria-disabled={disabled || undefined}
      className="flex min-w-0 flex-col gap-[12px]"
    >
      <PipelineSettings
        value={{
          name,
          mode: graph.mode ?? 'scheduled',
          signalType: graph.signalType,
          every,
          lookback,
          enabled,
        }}
        onChange={changeSettings}
        onModeChange={changeMode}
        onSignalChange={changeSignalType}
        disabled={disabled}
        disabledReason={disabledReason}
        enabledDisabled={enabledDisabled}
        enabledDisabledReason={enabledDisabledReason}
      />

      <section
        data-pipeline-flow-card
        aria-label={t('workspace.stage_graph')}
        className={cn('min-w-0 px-[16px] pb-[14px] pt-[12px]', surfacePanelClass)}
      >
        <header className="mb-[8px] flex min-h-[28px] items-center gap-[12px]">
          <h2 className="m-0 font-sans text-md font-strong text-tx-0">{t('workspace.stage_graph')}</h2>
          <FlowStatus issues={issues} urgent={heldBack} />
        </header>
        <PipelineFlow
          model={graph}
          connectorNames={resources.connectorNames}
          editing={{
            selectedId,
            onSelect: setSelectedId,
            onAddStep: addStep,
            onAddConnector: addConnector,
            issues,
            urgent: heldBack,
            disabled,
            disabledReason,
          }}
        />
        <div ref={issuesRef}>
          <FlowIssues issues={issues} urgent={heldBack} onSelect={setSelectedId} />
        </div>
      </section>

      <div className="grid min-w-0 gap-[12px] xl:grid-cols-[minmax(0,1.55fr)_minmax(360px,1fr)]">
        <NodeEditor
          model={graph}
          selectedId={selectedId}
          resources={resources}
          readOnly={disabled}
          onChange={setGraph}
          onSelect={setSelectedId}
        />
        <TestPanel model={graph} connectorNames={resources.connectorNames} />
      </div>
    </form>
  );
}
