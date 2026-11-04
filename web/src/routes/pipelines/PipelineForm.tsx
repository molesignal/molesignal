import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { PipelineInput, ScheduledPipeline } from '@/api/pipelines';
import { toApiError } from '@/lib/http';
import { uiSectionTitleClass } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { surfacePanelClass } from '@/shell/SurfaceWorkbench';

import { PipelineFlow, type DestinationKind } from './editor/Flow';
import { FlowIssues, FlowStatus } from './editor/Issues';
import { NodeEditor } from './editor/NodeEditor';
import { parseNodeId, removeNode } from './editor/nodes';
import { usePipelineResources } from './editor/resources';
import {
  formatInterval,
  intervalFromSeconds,
  intervalSeconds,
  parseInterval,
} from './editor/schedule';
import { PipelineSettings, type PipelineSettingsValue } from './editor/Settings';
import { newStep } from './editor/steps';
import { TestPanel } from './editor/test/TestPanel';
import {
  blocksSave,
  CONNECTOR_PREFIX,
  connectorOf,
  isStreamSink,
  pipelineGraphFromPipeline,
  pipelineInputFromGraph,
  TABLE_PREFIX,
  validateGraph,
  type PipelineGraphModel,
  type PipelineMode,
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
  /** Whether what is on the form differs from what was last saved. */
  onDirtyChange?: ((dirty: boolean) => void) | undefined;
  /** Goes up each time the page saved what is on the form. */
  savedRevision?: number | undefined;
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
  onDirtyChange,
  savedRevision = 0,
}: PipelineFormProps) {
  const { t } = useTranslation('pipelines');
  const resources = usePipelineResources();
  const [name, setName] = React.useState(initial?.name ?? '');
  const [every, setEvery] = React.useState(() => parseInterval(initial?.cron));
  const [lookback, setLookback] = React.useState(() => intervalFromSeconds(initial?.lookbackSecs));
  const [enabled, setEnabled] = React.useState(initial?.enabled ?? true);
  const [graph, setGraph] = React.useState<PipelineGraphModel>(() =>
    pipelineGraphFromPipeline(initial, 'logs'),
  );
  const [selectedId, setSelectedId] = React.useState(() => firstNode(graph));
  const [heldBack, setHeldBack] = React.useState(false);
  const issuesRef = React.useRef<HTMLDivElement>(null);

  const content = JSON.stringify({ name, every, lookback, enabled, graph });
  const [saved, setSaved] = React.useState(content);

  // eslint-disable-next-line react-hooks/exhaustive-deps -- a save settles the form as it stands at that moment
  React.useEffect(() => setSaved(content), [savedRevision]);

  const dirty = content !== saved;
  React.useEffect(() => onDirtyChange?.(dirty), [dirty, onDirtyChange]);

  const issues = React.useMemo(
    () => validateGraph(graph, resources.connectorIds),
    [graph, resources.connectorIds],
  );
  const blocked = blocksSave(issues);

  // Once the errors are gone there is nothing left to insist on.
  React.useEffect(() => {
    if (!blocked) setHeldBack(false);
  }, [blocked]);

  // A node that was removed, or a mode that dropped it, leaves the source picked. An alert
  // pipeline has one source and one destination, which are not in the lists.
  React.useEffect(() => {
    const node = parseNodeId(selectedId);
    const alert = graph.mode === 'alert';
    const count =
      node?.kind === 'source'
        ? alert ? 1 : graph.sources.length
        : node?.kind === 'sink'
          ? alert ? 1 : graph.sinks.length
          : graph.transforms.length;
    if (!node || node.index >= count) setSelectedId(SOURCE);
  }, [graph.mode, graph.sinks.length, graph.sources.length, graph.transforms.length, selectedId]);

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
    setGraph((current) => {
      // An alert pipeline has no stream on either side; one that turns back starts with a place for each.
      const sources = current.sources.length > 0 ? current.sources : [''];
      const sinks = current.sinks.length > 0 ? current.sinks : [''];

      if (mode === 'alert') {
        return { ...current, mode, sources: [], sinks: [] };
      }

      // Only a realtime pipeline takes profiles.
      if (mode === 'scheduled' && current.signalType === 'profiles') {
        return { ...current, mode, signalType: 'logs', sources: [''], sinks: [''] };
      }

      // Realtime works on data as it arrives: one stream in, and for logs one stream out, nothing
      // else; metrics and traces go back to the stream they came from.
      if (mode === 'realtime') {
        return {
          ...current,
          mode,
          sources: [sources[0] ?? ''],
          sinks: [sinks.find(isStreamSink) ?? ''],
          retainSource: current.retainSource ?? false,
        };
      }

      return { ...current, mode, sources, sinks };
    });
  };

  // Streams belong to a signal: another signal means another source and destination to choose.
  const changeSignalType = (signalType: PipelineSignalType) => {
    setGraph((current) => ({
      ...current,
      signalType,
      // Profiles arrive in one stream, whichever service sends them.
      sources: [signalType === 'profiles' ? 'default' : ''],
      sinks: ['', ...current.sinks.filter((sink) => !isStreamSink(sink))],
    }));
  };

  const changeSettings = (patch: Partial<PipelineSettingsValue>) => {
    if (patch.name !== undefined) setName(patch.name);
    if (patch.every !== undefined) setEvery(patch.every);
    if (patch.lookback !== undefined) setLookback(patch.lookback);
    if (patch.enabled !== undefined) setEnabled(patch.enabled);

    // How the pipeline reads and what it does when something fails is saved with its flow, and
    // so is an alert pipeline's place in the order.
    const { delaySecs, retry, deadLetterStream, dedupeField, order } = patch;
    if (
      delaySecs === undefined &&
      !retry &&
      deadLetterStream === undefined &&
      dedupeField === undefined &&
      order === undefined
    ) {
      return;
    }

    setGraph((current) => ({
      ...current,
      ...(delaySecs !== undefined && { delaySecs }),
      ...(retry && { retry }),
      ...(deadLetterStream !== undefined && { deadLetterStream }),
      ...(dedupeField !== undefined && { dedupeField }),
      ...(order !== undefined && { alert: { ...current.alert, order } }),
    }));
  };

  // A new source or destination starts unchosen: which one it is has to be said, not guessed.
  const addSource = () => {
    setSelectedId(`source-${graph.sources.length}`);
    setGraph({ ...graph, sources: [...graph.sources, ''] });
  };
  const addDestination = (kind: DestinationKind) => {
    const sink = kind === 'connector' ? CONNECTOR_PREFIX : kind === 'table' ? TABLE_PREFIX : '';
    // A new stream goes after the other streams, before the connectors and tables, as the flow lists
    // them; a connector after the connectors; a table at the end.
    const at =
      kind === 'table'
        ? graph.sinks.length
        : kind === 'connector'
          ? graph.sinks.filter((item) => connectorOf(item) !== null || isStreamSink(item)).length
          : graph.sinks.filter(isStreamSink).length;
    setSelectedId(`sink-${at}`);
    setGraph({ ...graph, sinks: [...graph.sinks.slice(0, at), sink, ...graph.sinks.slice(at)] });
  };
  const addStep = (type: StepType) => {
    const step = newStep(type, graph.transforms.map((item) => item.name), graph.mode, graph.sources[0]);
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
          delaySecs: graph.delaySecs,
          retry: graph.retry,
          deadLetterStream: graph.deadLetterStream,
          dedupeField: graph.dedupeField,
          stage: graph.alert.stage,
          order: graph.alert.order,
        }}
        resources={resources}
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
          <h2 className={cn('m-0', uiSectionTitleClass)}>{t('workspace.stage_graph')}</h2>
          <FlowStatus issues={issues} urgent={heldBack} />
        </header>
        <PipelineFlow
          model={graph}
          connectorNames={resources.connectorNames}
          editing={{
            selectedId,
            onSelect: setSelectedId,
            onRemove: (id) => {
              if (disabled) return;
              const next = removeNode(graph, id, selectedId);
              setGraph(next.model);
              setSelectedId(next.selectedId);
            },
            onAddStep: addStep,
            onAddSource: addSource,
            onAddDestination: addDestination,
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
          saved={initial ?? null}
          readOnly={disabled}
          onChange={setGraph}
          onSelect={setSelectedId}
        />
        <TestPanel model={graph} connectorNames={resources.connectorNames} />
      </div>
    </form>
  );
}
