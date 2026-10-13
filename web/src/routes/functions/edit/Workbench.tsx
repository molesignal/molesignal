import {
  CheckCircle2,
  CircleAlert,
  CircleDotDashed,
  Loader2,
  Play,
  WandSparkles,
  XCircle,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { FunctionLanguage } from '@/api/functions';
import type { ActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { CodeEditor } from '@/shell/codeEditor';
import { FormField, FormInput, FormSelect } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { surfacePanelClass } from '@/shell/SurfaceWorkbench';

export type ValidationKind = 'pending' | 'checking' | 'valid' | 'invalid';

export interface ValidationState {
  kind: ValidationKind;
  /** Why a save was refused. A failed sample run says its reason in the test card instead. */
  message?: string | undefined;
}

export type RunState =
  | { kind: 'idle' }
  | { kind: 'running' }
  | { kind: 'success'; durationMs: number }
  | { kind: 'error'; durationMs?: number | undefined; message: string };

/** The head of a card: its title, what state it is in, and its one action at the far end. */
const CARD_HEAD = 'flex min-h-[44px] items-center gap-[12px] px-[16px]';
const CARD_TITLE = 'm-0 font-sans text-md font-strong text-tx-0';
/** The head of a step inside the test card, a size down from the card's own. */
const STEP_HEAD = 'flex min-h-[36px] items-center gap-[8px] px-[16px]';
const STEP_TITLE = 'font-sans text-sm font-strong text-tx-1';
/** An editor set into a card: the card is its frame. */
const SET_IN = 'rounded-none border-0 shadow-none';

/**
 * Where a function is written and tried: what it is called and written in, its source, and a
 * sample event to run it on. Saving is the page's action and stands in the title row; running
 * the sample is this workbench's, and stands at the head of the card it fills.
 */
export function FunctionWorkbench({
  name,
  language,
  source,
  sampleInput,
  sampleOutput,
  validation,
  runState,
  outputPlaceholder,
  sampleInputError,
  writeAccess,
  runAccess,
  savePending,
  runPending,
  canRun,
  canSave,
  onSubmit,
  onNameChange,
  onLanguageChange,
  onSourceChange,
  onSampleInputChange,
  onFormatSource,
  onFormatInput,
  onRun,
  onSave,
}: {
  name: string;
  language: FunctionLanguage;
  source: string;
  sampleInput: string;
  sampleOutput: string;
  validation: ValidationState;
  runState: RunState;
  outputPlaceholder: string;
  sampleInputError: string | null;
  writeAccess: ActionAccess;
  runAccess: ActionAccess;
  savePending: boolean;
  runPending: boolean;
  canRun: boolean;
  canSave: boolean;
  onSubmit: React.FormEventHandler<HTMLFormElement>;
  onNameChange: (value: string) => void;
  onLanguageChange: (value: string) => void;
  onSourceChange: (value: string) => void;
  onSampleInputChange: (value: string) => void;
  onFormatSource: () => void;
  onFormatInput: () => void;
  onRun: () => void;
  onSave: () => void;
}) {
  const { t } = useTranslation('functions');
  const editorLanguage = language === 'vrl' ? 'vrl' : 'javascript';
  const sourceTitleId = React.useId();
  const testTitleId = React.useId();

  return (
    <form
      id="function-form"
      onSubmit={onSubmit}
      className="flex min-w-0 flex-col gap-[12px]"
    >
      <section
        data-function-definition
        aria-label={t('edit.definition')}
        className={cn('px-[16px] py-[14px]', surfacePanelClass)}
      >
        <div className="grid grid-cols-1 gap-x-[16px] gap-y-[12px] md:grid-cols-[minmax(0,1fr)_220px]">
          <FormField label={t('edit.name_label')} required>
            <FormInput
              value={name}
              disabled={writeAccess.disabled || savePending}
              disabledReason={writeAccess.reason}
              onChange={(event) => onNameChange(event.target.value)}
              placeholder={t('edit.name_placeholder')}
              autoComplete="off"
              required
            />
          </FormField>
          <FormField label={t('edit.language_label')} required>
            <FormSelect
              value={language}
              disabled={writeAccess.disabled || savePending}
              disabledReason={writeAccess.reason}
              onChange={onLanguageChange}
              options={[
                { value: 'vrl', label: 'VRL' },
                { value: 'js', label: t('edit.languages.javascript') },
              ]}
            />
          </FormField>
        </div>
      </section>

      <div
        data-function-workbench
        className="grid min-w-0 grid-cols-1 gap-[12px] xl:grid-cols-[minmax(0,2.1fr)_minmax(360px,1fr)]"
      >
        <section
          data-function-source
          aria-labelledby={sourceTitleId}
          className={cn('flex min-w-0 flex-col overflow-hidden', surfacePanelClass)}
        >
          <header className={CARD_HEAD}>
            <h2 id={sourceTitleId} className={CARD_TITLE}>
              {t('edit.source_label')}
            </h2>
            <span aria-live="polite">
              <WorkbenchStatus kind={validation.kind}>
                {t(`edit.validation.${validation.kind}`)}
              </WorkbenchStatus>
            </span>
            <ChromeButton
              size="sm"
              className="ml-auto"
              disabled={writeAccess.disabled}
              disabledReason={writeAccess.reason}
              onClick={onFormatSource}
            >
              <WandSparkles className="h-3.5 w-3.5" />
              {t('edit.format')}
            </ChromeButton>
          </header>
          {validation.kind === 'invalid' && validation.message && (
            <div
              data-function-validation-detail
              role="status"
              className="flex items-start gap-2 bg-red-dim px-[16px] py-2 font-sans text-xs leading-relaxed text-red-soft"
            >
              <StatusIcon kind="invalid" />
              <span className="min-w-0 break-words">{validation.message}</span>
            </div>
          )}
          <CodeEditor
            value={source}
            onChange={onSourceChange}
            readOnly={writeAccess.disabled}
            language={editorLanguage}
            ariaLabel={t('edit.source_label')}
            minHeight={460}
            maxHeight={720}
            onModEnter={onRun}
            onModSave={() => canSave && onSave()}
            resizable
            showHeader={false}
            showStatus={false}
            className={SET_IN}
          />
        </section>

        <section
          data-function-test
          aria-labelledby={testTitleId}
          className={cn('flex min-w-0 flex-col overflow-hidden', surfacePanelClass)}
        >
          <header className={CARD_HEAD}>
            <h2 id={testTitleId} className={CARD_TITLE}>
              {t('edit.test_runner')}
            </h2>
            <ChromeButton
              size="sm"
              className="ml-auto"
              data-function-run=""
              onClick={onRun}
              disabled={!canRun}
              disabledReason={runAccess.reason}
            >
              {runPending ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin motion-reduce:animate-none" />
              ) : (
                <Play className="h-3.5 w-3.5" />
              )}
              {runPending ? t('edit.running') : t('edit.run_sample')}
            </ChromeButton>
          </header>

          <div className={STEP_HEAD}>
            <span className={STEP_TITLE}>{t('edit.sample_input')}</span>
            {/* Said only when it is wrong: valid input is what the editor shows. */}
            {sampleInputError && (
              <span
                data-function-input-invalid
                className="inline-flex items-center gap-1.5 font-sans text-xs font-strong text-red-soft"
              >
                <CircleAlert className="h-3.5 w-3.5" />
                {t('edit.json_invalid')}
              </span>
            )}
            <ChromeButton
              size="sm"
              className="ml-auto"
              onClick={onFormatInput}
              disabled={runAccess.disabled || sampleInputError !== null}
              disabledReason={runAccess.reason}
            >
              <WandSparkles className="h-3.5 w-3.5" />
              {t('edit.format')}
            </ChromeButton>
          </div>
          <CodeEditor
            value={sampleInput}
            onChange={onSampleInputChange}
            readOnly={runAccess.disabled}
            language="json"
            ariaLabel={t('edit.sample_input')}
            minHeight={180}
            maxHeight={280}
            onModEnter={onRun}
            showHeader={false}
            showStatus={false}
            className={SET_IN}
          />

          <div className={STEP_HEAD}>
            <span className={STEP_TITLE}>{t('edit.sample_output')}</span>
            <div className="ml-auto" aria-live="polite">
              <RunStatus state={runState} />
            </div>
          </div>
          {runState.kind === 'error' ? (
            <div data-function-run-error className="min-h-[220px] px-[16px] pb-[16px]">
              <div className="flex items-start gap-2 rounded-md bg-red-dim p-3">
                <XCircle className="mt-0.5 h-4 w-4 shrink-0 text-red-soft" />
                <div className="min-w-0">
                  <div className="font-sans text-xs font-strong text-red-soft">
                    {t('edit.run_failed')}
                  </div>
                  <pre className="m-0 mt-2 whitespace-pre-wrap break-words font-code text-xs leading-relaxed text-tx-1">
                    {runState.message}
                  </pre>
                </div>
              </div>
            </div>
          ) : (
            <CodeEditor
              value={sampleOutput}
              language="json"
              ariaLabel={t('edit.sample_output')}
              placeholder={outputPlaceholder}
              readOnly
              minHeight={220}
              maxHeight={320}
              showHeader={false}
              showStatus={false}
              className={SET_IN}
            />
          )}
        </section>
      </div>
    </form>
  );
}

const STATUS_TONE: Record<ValidationKind, string> = {
  valid: 'text-[var(--green-ink)]',
  invalid: 'text-red-soft',
  checking: 'text-[var(--blue-ink)]',
  pending: 'text-tx-3',
};

/** A state said in a mark and a word, in the tone of the state: no box around it. */
function WorkbenchStatus({
  kind,
  children,
}: {
  kind: ValidationKind;
  children: React.ReactNode;
}) {
  return (
    <span
      data-function-status={kind}
      className={cn(
        'inline-flex items-center gap-1.5 whitespace-nowrap font-sans text-xs font-strong',
        STATUS_TONE[kind],
      )}
    >
      <StatusIcon kind={kind} />
      {children}
    </span>
  );
}

function StatusIcon({ kind }: { kind: ValidationKind }) {
  if (kind === 'valid') {
    return <CheckCircle2 className="h-3.5 w-3.5 shrink-0" />;
  }
  if (kind === 'invalid') {
    return <XCircle className="h-3.5 w-3.5 shrink-0" />;
  }
  if (kind === 'checking') {
    return (
      <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin motion-reduce:animate-none" />
    );
  }
  return <CircleDotDashed className="h-3.5 w-3.5 shrink-0" />;
}

function RunStatus({ state }: { state: RunState }) {
  const { t } = useTranslation('functions');

  if (state.kind === 'success') {
    return (
      <WorkbenchStatus kind="valid">
        {t('edit.run_success', { duration: state.durationMs })}
      </WorkbenchStatus>
    );
  }
  if (state.kind === 'error') {
    return (
      <WorkbenchStatus kind="invalid">
        {state.durationMs
          ? t('edit.run_error_timed', { duration: state.durationMs })
          : t('edit.run_error')}
      </WorkbenchStatus>
    );
  }
  if (state.kind === 'running') {
    return (
      <WorkbenchStatus kind="checking">
        {t('edit.run_running')}
      </WorkbenchStatus>
    );
  }
  return (
    <WorkbenchStatus kind="pending">{t('edit.run_idle')}</WorkbenchStatus>
  );
}
