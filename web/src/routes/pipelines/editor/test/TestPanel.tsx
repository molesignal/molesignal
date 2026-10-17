import {
  CircleCheck,
  CircleSlash,
  CircleX,
  Database,
  LoaderCircle,
  MinusCircle,
  Play,
  RotateCcw,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as functionsApi from '@/api/functions';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton, uiLabelClass } from '@/shell/chrome';
import { CodeEditor } from '@/shell/codeEditor';
import { FormSelect } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { surfacePanelClass } from '@/shell/SurfaceWorkbench';
import { useCurrentOrgSelection } from '@/stores/useOrgStore';

import { recentEvent } from './recentEvent';
import { runPipelineTest, type StepOutcome, type TestOutcome } from './run';
import { sampleEvent } from './sample';
import { connectorOf, type PipelineGraphModel } from '../../PipelineGraph/model';

type RunState =
  | { kind: 'idle' }
  | { kind: 'running' }
  | { kind: 'done'; run: number; outcome: TestOutcome; steps: string };

/** What the steps of a pipeline are, as far as a test run is concerned. */
function stepsKey(model: PipelineGraphModel): string {
  return JSON.stringify(model.transforms);
}

/**
 * Tries the pipeline on one event before it is saved: the event goes through
 * every step on the backend's own runtime and comes back as it would be
 * written, with where it would go, or with the step that dropped it. With
 * several sources, the sample is said to come from one of them, which the
 * steps see as `%source_stream`. Nothing is stored.
 */
export function TestPanel({
  model,
  connectorNames,
  className,
}: {
  model: PipelineGraphModel;
  connectorNames: Readonly<Record<string, string>>;
  className?: string;
}) {
  const { t } = useTranslation('pipelines');
  const runAccess = useActionAccess({ permission: 'functions.run' });
  const { currentOrgId } = useCurrentOrgSelection();
  const [sample, setSample] = React.useState(() => sampleEvent(model.signalType));
  const [state, setState] = React.useState<RunState>({ kind: 'idle' });
  const runs = React.useRef(0);
  const [loading, setLoading] = React.useState(false);
  const [sampleNote, setSampleNote] = React.useState<string | null>(null);
  const sources = [...new Set(model.sources.map((item) => item.trim()).filter(Boolean))];
  const [chosenSource, setChosenSource] = React.useState('');
  // The source picked for the sample while it is still one; the first one otherwise.
  const source = sources.includes(chosenSource) ? chosenSource : (sources[0] ?? '');

  // Another signal is another shape of event; a sample nobody edited follows it.
  const signalRef = React.useRef(model.signalType);
  React.useEffect(() => {
    if (signalRef.current === model.signalType) return;

    setSample((current) =>
      current === sampleEvent(signalRef.current) ? sampleEvent(model.signalType) : current,
    );
    signalRef.current = model.signalType;
  }, [model.signalType]);

  const input = React.useMemo(() => {
    try {
      return { value: JSON.parse(sample.trim() || '{}') as unknown };
    } catch {
      return null;
    }
  }, [sample]);

  const run = async () => {
    if (!input || runAccess.disabled) return;

    const steps = stepsKey(model);
    runs.current += 1;
    setState({ kind: 'running' });
    const outcome = await runPipelineTest(
      model,
      input.value,
      async (script, event, from) => {
        const response = await functionsApi.run({
          language: 'vrl',
          source: script,
          input: event,
          metadata: { source_stream: from },
        });
        return { output: response.output, dropped: response.dropped === true };
      },
      source,
    );
    setState({ kind: 'done', run: runs.current, outcome, steps });
  };

  // A sample made up here says what a step does; one the stream really holds says what it will do.
  const loadRecent = async () => {
    if (!source || !currentOrgId) return;

    setLoading(true);
    setSampleNote(null);
    try {
      const event = await recentEvent(currentOrgId, source, model.signalType);
      if (event) setSample(JSON.stringify(event, null, 2));
      else setSampleNote(t('test.no_recent', { stream: source }));
    } catch (error) {
      setSampleNote(toApiError(error).message);
    } finally {
      setLoading(false);
    }
  };

  const stale = state.kind === 'done' && state.steps !== stepsKey(model);

  return (
    <section
      data-pipeline-test
      aria-label={t('test.title')}
      className={cn('flex min-w-0 flex-col px-[16px] pb-[16px] pt-[12px]', surfacePanelClass, className)}
    >
      <header className="flex min-h-[36px] items-center gap-[10px]">
        <h2 className="m-0 font-sans text-md font-strong text-tx-0">{t('test.title')}</h2>
        <span className="min-w-0 flex-1 truncate font-sans text-xs text-tx-3">{t('test.subtitle')}</span>
        <ChromeButton
          size="sm"
          disabled={runAccess.disabled || !input || state.kind === 'running'}
          disabledReason={runAccess.disabled ? runAccess.reason : undefined}
          onClick={() => void run()}
        >
          {state.kind === 'running' ? (
            <LoaderCircle aria-hidden className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <Play aria-hidden className="h-3.5 w-3.5" />
          )}
          {t(state.kind === 'running' ? 'test.running' : 'test.run')}
        </ChromeButton>
      </header>

      <div className="mt-[10px] flex min-w-0 items-center gap-[12px]">
        <span className={cn('mr-auto shrink-0 whitespace-nowrap', uiLabelClass)}>{t('test.sample')}</span>
        {sources.length > 1 && (
          <div className="w-[150px] min-w-0">
            <FormSelect
              value={source}
              onChange={setChosenSource}
              ariaLabel={t('test.sample_source')}
              options={sources.map((name) => ({ value: name, label: t('test.from_source', { stream: name }) }))}
            />
          </div>
        )}
        {source && (
          <SampleAction disabled={loading} onClick={() => void loadRecent()}>
            {loading ? (
              <LoaderCircle aria-hidden className="h-3 w-3 shrink-0 animate-spin" />
            ) : (
              <Database aria-hidden className="h-3 w-3 shrink-0" />
            )}
            <span className="truncate">{t('test.use_recent', { stream: source })}</span>
          </SampleAction>
        )}
        {sample !== sampleEvent(model.signalType) && (
          <SampleAction
            onClick={() => {
              setSample(sampleEvent(model.signalType));
              setSampleNote(null);
            }}
          >
            <RotateCcw aria-hidden className="h-3 w-3 shrink-0" />
            {t('test.reset_sample')}
          </SampleAction>
        )}
      </div>
      <CodeEditor
        className="mt-[6px]"
        value={sample}
        onChange={setSample}
        language="json"
        ariaLabel={t('test.sample')}
        minHeight={150}
        maxHeight={300}
        showHeader={false}
        showStatus={false}
        lineNumbers={false}
        resizable
        onModEnter={() => void run()}
      />
      {!input && (
        <p role="alert" className="m-0 mt-[6px] font-sans text-xs text-red-soft">
          {t('test.sample_invalid')}
        </p>
      )}
      {sampleNote && (
        <p role="status" className="m-0 mt-[6px] font-sans text-xs leading-relaxed text-tx-3">
          {sampleNote}
        </p>
      )}

      {state.kind === 'done' ? (
        <TestResult
          // A new run starts on its own result, not on the step picked in the last one.
          key={state.run}
          outcome={state.outcome}
          stale={stale}
          connectorNames={connectorNames}
        />
      ) : (
        <p className="m-0 mt-[14px] font-sans text-xs leading-relaxed text-tx-3">{t('test.hint')}</p>
      )}
    </section>
  );
}

/** A quiet action beside the sample's caption. */
function SampleAction({
  disabled = false,
  onClick,
  children,
}: {
  disabled?: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      className="inline-flex min-w-0 items-center gap-1 rounded font-sans text-xs text-tx-3 outline-none transition-colors enabled:hover:text-tx-0 enabled:focus-visible:text-tx-0 enabled:focus-visible:underline disabled:cursor-default"
    >
      {children}
    </button>
  );
}

function TestResult({
  outcome,
  stale,
  connectorNames,
}: {
  outcome: TestOutcome;
  stale: boolean;
  connectorNames: Readonly<Record<string, string>>;
}) {
  const { t } = useTranslation('pipelines');
  const failed = outcome.steps.some((step) => step.status === 'failed');
  const dropper = outcome.steps.find((step) => step.status === 'dropped');
  const passed = outcome.steps.filter((step) => step.status === 'passed');
  // A dropped event is shown as it came to the step that dropped it.
  const last = dropper ?? passed.at(-1);
  // The event is shown as the last step left it, or as any earlier one did when that one is picked.
  const [picked, setPicked] = React.useState<number | null>(null);
  const captionId = React.useId();
  const shown = passed.find((step) => step.index === picked) ?? last;
  // With no steps there is no step to have left it: the event is what was put in.
  const untouched = outcome.steps.length === 0;
  const final = !failed && !dropper && shown === last;
  const event = untouched ? { output: outcome.output } : shown;
  const destinations = outcome.destinations.map((destination) => {
    const connector = connectorOf(destination);
    return connector === null ? destination : (connectorNames[connector] ?? connector);
  });

  return (
    <div
      data-pipeline-test-result={failed ? 'failed' : dropper ? 'dropped' : 'passed'}
      data-stale={stale || undefined}
      className={cn('mt-[14px] min-w-0 transition-opacity', stale && 'opacity-60')}
    >
      {stale && (
        <p className="m-0 mb-[8px] font-sans text-xs text-yellow">{t('test.stale')}</p>
      )}
      {untouched && (
        <p className="m-0 font-sans text-xs leading-relaxed text-tx-3">{t('test.no_steps')}</p>
      )}
      <ol className="m-0 list-none space-y-[2px] p-0">
        {outcome.steps.map((step) => (
          <StepResult
            key={step.index}
            step={step}
            shown={step === shown}
            onShow={step.status === 'passed' || step.status === 'dropped' ? () => setPicked(step.index) : undefined}
          />
        ))}
      </ol>

      {event && (
        <>
          <div className="mt-[12px] flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
            <span id={captionId} className={uiLabelClass}>
              {final || !shown
                ? t('test.output')
                : shown === dropper
                  ? t('test.dropped_at', { number: shown.index + 1 })
                  : t('test.after_step', { number: shown.index + 1 })}
            </span>
            {final && destinations.length > 0 && (
              <span className="min-w-0 truncate font-sans text-xs text-tx-3">
                {t('test.written_to')}{' '}
                <span className="font-mono font-strong text-tx-1">{destinations.join(', ')}</span>
              </span>
            )}
          </div>
          <pre
            data-pipeline-test-output
            // A long event scrolls; the keyboard reaches it like any other region.
            tabIndex={0}
            aria-labelledby={captionId}
            className="m-0 mt-[6px] max-h-[320px] overflow-auto rounded-md bg-bg-0 px-4 py-3 font-code text-xs leading-[1.7] text-tx-1 transition-colors duration-fast focus-visible:bg-bg-2"
          >
            {JSON.stringify(event.output, null, 2)}
          </pre>
        </>
      )}
      {failed && (
        <p className="m-0 mt-[10px] font-sans text-xs leading-relaxed text-tx-3">{t('test.failed_note')}</p>
      )}
      {dropper && (
        <p className="m-0 mt-[10px] font-sans text-xs leading-relaxed text-tx-3">{t('test.dropped_note')}</p>
      )}
    </div>
  );
}

function StepResult({
  step,
  shown,
  onShow,
}: {
  step: StepOutcome;
  /** This step's event is the one on view below. */
  shown: boolean;
  onShow: (() => void) | undefined;
}) {
  const { t } = useTranslation('pipelines');
  const Icon =
    step.status === 'passed'
      ? CircleCheck
      : step.status === 'failed'
        ? CircleX
        : step.status === 'dropped'
          ? CircleSlash
          : MinusCircle;
  const row = 'flex w-full min-w-0 items-start gap-[8px] rounded px-[6px] py-[3px] text-left font-sans text-xs';
  const body = (
    <>
      <Icon
        aria-hidden
        className={cn(
          'mt-[1px] h-3.5 w-3.5 shrink-0',
          step.status === 'passed' && 'text-green',
          step.status === 'failed' && 'text-red-soft',
          step.status === 'dropped' && 'text-tx-2',
          step.status === 'skipped' && 'text-tx-3',
        )}
      />
      <span className="min-w-0">
        <span className={cn('font-strong', step.status === 'skipped' ? 'text-tx-3' : 'text-tx-1')}>
          <span className="mr-[6px] font-mono tabular-nums text-tx-3">{step.index + 1}</span>
          {step.name || t('flow.unnamed_step')}
        </span>
        {step.destination !== undefined && (
          <span className="text-tx-3">
            {' → '}
            <span className="font-mono text-tx-1">{step.destination}</span>
          </span>
        )}
        {step.status === 'skipped' && <span className="text-tx-3"> · {t('test.skipped')}</span>}
        {step.status === 'dropped' && <span className="text-tx-2"> · {t('test.dropped')}</span>}
        {step.failure && (
          <span className="mt-[2px] block leading-relaxed text-red-soft">
            {step.failure.kind === 'runtime'
              ? t('test.failure_runtime')
              : step.failure.kind === 'compile'
                ? t('test.failure_compile', { message: step.failure.message })
                : step.failure.message}
          </span>
        )}
      </span>
    </>
  );

  return (
    <li data-test-step={step.status} className="-mx-[6px]">
      {onShow ? (
        <button
          type="button"
          aria-pressed={shown}
          onClick={onShow}
          className={cn(
            row,
            'outline-none transition-colors duration-fast',
            shown ? 'bg-[var(--control-surface)]' : 'hover:bg-bg-2 focus-visible:bg-bg-2',
          )}
        >
          {body}
        </button>
      ) : (
        <div className={row}>{body}</div>
      )}
    </li>
  );
}
