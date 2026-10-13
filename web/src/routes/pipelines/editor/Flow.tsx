import { Database, Plug, Plus, Send, type LucideIcon } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ChromeButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';

import { STEP_ICONS } from './steps';
import {
  connectorOf,
  stepType,
  writtenSinkIndex,
  type PipelineGraphModel,
  type StepType,
} from '../PipelineGraph/model';
import type { GraphIssue } from '../PipelineGraph/validation';
import { PROCESSOR_GROUPS, processorSummary } from '../processors/catalog';

interface FlowNode {
  id: string;
  icon: LucideIcon;
  title: string;
  /** Set when the node has no name yet: the title is then a prompt, not a value. */
  placeholder?: boolean;
  detail: string;
  ordinal?: number;
  /** Saved with the pipeline but no part of what runs: a stream that is never read or written. */
  idle?: boolean;
}

export interface FlowEditing {
  selectedId: string;
  onSelect: (id: string) => void;
  onAddStep: (type: StepType) => void;
  onAddConnector: () => void;
  issues: readonly GraphIssue[];
  /** A save was held back: what is not filled in yet is now an error to look at. */
  urgent?: boolean;
  disabled?: boolean;
  disabledReason?: string | undefined;
}

/**
 * A pipeline drawn the way it runs: the stream it reads, the steps one after
 * another, and where the events go, which is one stream and any number of
 * connectors. That is the whole of what a pipeline can be, so there is
 * nothing to drag or to wire: a node is picked to edit it, and a step or a
 * connector is added at the end of its lane.
 *
 * Without `editing` it is a picture of a saved pipeline.
 */
export function PipelineFlow({
  model,
  connectorNames = {},
  editing,
  className,
}: {
  model: PipelineGraphModel;
  connectorNames?: Readonly<Record<string, string>>;
  editing?: FlowEditing | undefined;
  className?: string;
}) {
  const { t } = useTranslation('pipelines');
  const routed = model.transforms.some((step) => step.kind === 'builtin');
  const single = model.mode === 'realtime';
  const written = writtenSinkIndex(model.sinks);

  const sources: FlowNode[] = model.sources.map((source, index) => ({
    id: `source-${index}`,
    icon: Database,
    title: source.trim() || t('flow.choose_stream'),
    placeholder: !source.trim(),
    detail: index > 0 ? t('flow.not_read') : t('graph.node_subtitles.stream_input'),
    idle: index > 0,
  }));

  const steps: FlowNode[] = model.transforms.map((step, index) => {
    const type = stepType(step);
    const summary = step.processor ? processorSummary(step.processor) : '';

    return {
      id: `transform-${index}`,
      icon: STEP_ICONS[type],
      title: step.name.trim() || t('flow.unnamed_step'),
      placeholder: !step.name.trim(),
      detail: [t(`steps.types.${type}`), summary].filter(Boolean).join(' · '),
      ordinal: index + 1,
    };
  });

  const sinks: FlowNode[] = model.sinks.map((sink, index) => {
    const id = `sink-${index}`;
    const connector = connectorOf(sink);

    if (connector !== null) {
      return {
        id,
        icon: Plug,
        title: connector ? (connectorNames[connector] ?? connector) : t('flow.choose_connector'),
        placeholder: !connector,
        detail: t('graph.node_subtitles.connector_sink'),
      };
    }
    if (index !== written) {
      return { id, icon: Send, title: sink.trim(), detail: t('flow.not_written'), idle: true };
    }
    // A routing step decides the stream; the destination node stands for whatever it picks.
    if (routed) {
      return { id, icon: Send, title: t('processing.output'), detail: t('flow.routed_detail') };
    }
    return {
      id,
      icon: Send,
      title: sink.trim() || t('flow.choose_stream'),
      placeholder: !sink.trim(),
      detail: t('graph.node_subtitles.stream_output'),
    };
  });

  const lane = useScrollEdges(steps.length);
  const node = (item: FlowNode, className?: string) => (
    <FlowNodeCard key={item.id} node={item} editing={editing} className={className} />
  );
  const add = (label: string, onClick: () => void) =>
    editing && (
      <ChromeButton
        size="sm"
        variant="ghost"
        className="justify-self-start text-tx-2"
        disabled={editing.disabled}
        disabledReason={editing.disabledReason}
        onClick={onClick}
      >
        <Plus aria-hidden className="h-3.5 w-3.5" />
        {label}
      </ChromeButton>
    );

  return (
    <div
      data-pipeline-flow
      className={cn(
        'grid grid-cols-[minmax(150px,220px)_40px_minmax(0,1fr)_40px_minmax(150px,220px)] gap-y-[8px]',
        className,
      )}
    >
      <LaneLabel className="col-start-1">{t('flow.source')}</LaneLabel>
      <LaneLabel className="col-start-3">{t('flow.processing')}</LaneLabel>
      <LaneLabel className="col-start-5">{t('flow.destinations')}</LaneLabel>

      <div
        role="group"
        aria-label={t('flow.source')}
        className="col-start-1 row-start-2 flex min-w-0 flex-col gap-[6px] self-center"
      >
        {sources.map((item) => node(item))}
      </div>
      <FlowLink className="col-start-2 row-start-2 self-center justify-self-center" />
      <div
        ref={lane.ref}
        data-flow-lane
        role="group"
        aria-label={t('flow.processing')}
        // A long chain scrolls sideways; the edge with more steps beyond it fades to say so.
        className="col-start-3 row-start-2 flex min-w-0 items-center self-center overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
        style={lane.style}
      >
        {steps.map((item, index) => (
          <React.Fragment key={item.id}>
            {index > 0 && <FlowLink className="mx-[2px] w-5" />}
            {node(item, 'min-w-[148px] max-w-[220px] flex-[1_1_196px]')}
          </React.Fragment>
        ))}
        {steps.length === 0 && (
          <span data-flow-no-steps className="shrink-0 font-sans text-xs text-tx-3">
            {t('flow.no_steps')}
          </span>
        )}
        {/* The line runs on from the last step to the arrow into the destinations. */}
        <span aria-hidden className="ml-[8px] h-px min-w-[8px] flex-1 bg-tx-3" />
      </div>
      <FlowLink className="col-start-4 row-start-2 self-center justify-self-center" />
      <div
        role="group"
        aria-label={t('flow.destinations')}
        className="col-start-5 row-start-2 flex min-w-0 flex-col gap-[6px] self-center"
      >
        {sinks.map((item) => node(item))}
      </div>

      {editing && (
        <>
          <div className="col-start-3 row-start-3 grid">
            <AddStepMenu editing={editing} />
          </div>
          <div className="col-start-5 row-start-3 grid">
            {/* A realtime pipeline writes to its stream and nowhere else. */}
            {!single && add(t('flow.add_connector'), editing.onAddConnector)}
          </div>
        </>
      )}
    </div>
  );
}

const FADE = 28;

/**
 * Which edges of a sideways scroller have more beyond them, as a mask that
 * fades those edges: a card is never cut off without a hint that there is more.
 */
function useScrollEdges(items: number) {
  const ref = React.useRef<HTMLDivElement>(null);
  const [edges, setEdges] = React.useState({ start: false, end: false });

  React.useLayoutEffect(() => {
    const scroller = ref.current;
    if (!scroller) return;

    const update = () => {
      const next = {
        start: scroller.scrollLeft > 1,
        end: scroller.scrollLeft + scroller.clientWidth < scroller.scrollWidth - 1,
      };
      setEdges((previous) =>
        previous.start === next.start && previous.end === next.end ? previous : next,
      );
    };
    update();
    scroller.addEventListener('scroll', update, { passive: true });
    const observer = new ResizeObserver(update);
    observer.observe(scroller);
    for (const child of scroller.children) observer.observe(child);

    return () => {
      scroller.removeEventListener('scroll', update);
      observer.disconnect();
    };
    // The cards are observed, so the edges are worked out again when one is added or removed.
  }, [items]);

  const mask = `linear-gradient(to right, transparent 0, #000 ${edges.start ? FADE : 0}px, #000 calc(100% - ${edges.end ? FADE : 0}px), transparent 100%)`;
  const style: React.CSSProperties =
    edges.start || edges.end ? { maskImage: mask, WebkitMaskImage: mask } : {};

  return { ref, style, edges };
}

function LaneLabel({ className, children }: { className: string; children: React.ReactNode }) {
  return (
    <div className={cn('row-start-1 font-sans text-xs font-strong text-tx-3', className)}>
      {children}
    </div>
  );
}

/** The line from one node to the next. */
function FlowLink({ className }: { className?: string }) {
  return (
    <svg
      aria-hidden
      data-flow-link
      viewBox="0 0 32 12"
      preserveAspectRatio="none"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.25"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={cn('h-3 w-8 shrink-0 text-tx-3', className)}
    >
      <path d="M1 6h29" vectorEffect="non-scaling-stroke" />
      <path d="M26 2.5 30.5 6 26 9.5" vectorEffect="non-scaling-stroke" />
    </svg>
  );
}

function FlowNodeCard({
  node,
  editing,
  className,
}: {
  node: FlowNode;
  editing?: FlowEditing | undefined;
  className?: string | undefined;
}) {
  const { t } = useTranslation('pipelines');
  const Icon = node.icon;
  const selected = editing?.selectedId === node.id;
  const issues = editing?.issues.filter((issue) => issue.node === node.id) ?? [];
  const errors = issues.filter((issue) => issue.level === 'error');
  // Something not filled in yet is work to do, and a mistake only once a save was tried.
  const broken = errors.some((issue) => !issue.pending || editing?.urgent);
  const mark = broken ? 'error' : errors.length > 0 ? 'pending' : issues.length > 0 ? 'warning' : null;
  const ref = React.useRef<HTMLButtonElement>(null);

  // A step picked from elsewhere (just added, or named by an issue) may sit beyond the lane's
  // edge, or under its fade. It is then brought to the middle; one already in clear view stays put.
  React.useEffect(() => {
    const card = ref.current;
    if (!selected || !card) return;

    const lane = card.closest('[data-flow-lane]')?.getBoundingClientRect();
    const own = card.getBoundingClientRect();
    const clear = lane && own.left >= lane.left + FADE && own.right <= lane.right - FADE;
    if (!clear) card.scrollIntoView?.({ block: 'nearest', inline: lane ? 'center' : 'nearest' });
  }, [selected]);

  const body = (
    <>
      <span
        aria-hidden
        className={cn(
          'relative grid h-[28px] w-[28px] shrink-0 place-items-center rounded-md transition-colors duration-fast',
          selected ? 'bg-indigo/15 text-indigo-soft' : 'bg-[var(--functional-surface)] text-tx-2',
        )}
      >
        <Icon className="h-3.5 w-3.5" strokeWidth={1.8} />
      </span>
      <span className="min-w-0 flex-1">
        <span className="flex min-w-0 items-baseline gap-[6px]">
          {node.ordinal !== undefined && (
            <span className="shrink-0 font-mono text-xs tabular-nums text-tx-3">{node.ordinal}</span>
          )}
          <span
            className={cn(
              'min-w-0 truncate font-sans text-sm',
              node.placeholder
                ? 'font-body text-tx-3'
                : node.idle
                  ? 'font-body text-tx-2'
                  : 'font-strong text-tx-0',
            )}
          >
            {node.title}
          </span>
        </span>
        <span className="block truncate font-sans text-xs text-tx-3">{node.detail}</span>
      </span>
      {mark && (
        <span
          aria-hidden
          data-flow-node-issue={mark}
          className={cn(
            'absolute right-[8px] top-[8px] h-[6px] w-[6px] rounded-full',
            mark === 'error' ? 'bg-red' : mark === 'warning' ? 'bg-yellow' : 'bg-tx-3',
          )}
        />
      )}
    </>
  );
  const shape = cn(
    'relative flex min-h-[52px] min-w-0 items-center gap-[10px] rounded-md px-[10px] py-[8px] text-left',
    className,
  );

  if (!editing) {
    return (
      <div
        data-pipeline-node={node.id}
        data-idle={node.idle || undefined}
        className={cn(shape, 'bg-[var(--control-surface)]')}
      >
        {body}
      </div>
    );
  }

  return (
    <button
      ref={ref}
      type="button"
      data-pipeline-node={node.id}
      data-idle={node.idle || undefined}
      aria-pressed={selected}
      aria-invalid={broken || undefined}
      aria-label={
        issues.length > 0
          ? t('flow.node_with_issues', { name: node.title, count: issues.length })
          : undefined
      }
      onClick={() => editing.onSelect(node.id)}
      className={cn(
        shape,
        'outline-none transition-colors duration-fast',
        selected
          ? 'bg-indigo-dim'
          : 'bg-[var(--control-surface)] hover:bg-bg-3 focus-visible:bg-bg-3',
      )}
    >
      {body}
    </button>
  );
}

/** What a new step can be: a ready-made processor, a route, or VRL written by hand. */
function AddStepMenu({ editing }: { editing: FlowEditing }) {
  const { t } = useTranslation('pipelines');
  const item = (type: StepType) => {
    const Icon = STEP_ICONS[type];

    return (
      <DropdownMenuItem key={type} onSelect={() => editing.onAddStep(type)}>
        <Icon aria-hidden className="h-3.5 w-3.5 text-tx-2" />
        {t(`steps.types.${type}`)}
      </DropdownMenuItem>
    );
  };

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <ChromeButton
          size="sm"
          variant="ghost"
          className="justify-self-start text-tx-2"
          disabled={editing.disabled}
          disabledReason={editing.disabledReason}
        >
          <Plus aria-hidden className="h-3.5 w-3.5" />
          {t('flow.add_step')}
        </ChromeButton>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="min-w-[208px]">
        {PROCESSOR_GROUPS.map(({ group, ops }) => (
          <React.Fragment key={group}>
            <DropdownMenuLabel>{t(`steps.groups.${group}`)}</DropdownMenuLabel>
            {ops.map(item)}
          </React.Fragment>
        ))}
        <DropdownMenuSeparator />
        {item('route')}
        {item('vrl')}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
