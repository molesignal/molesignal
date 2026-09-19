import {
  CheckCircle2,
  CircleAlert,
  CircleDotDashed,
  Code2,
  Loader2,
  Play,
  WandSparkles,
  XCircle,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { FunctionLanguage } from '@/api/functions';
import type { ActionAccess } from '@/product/actionAccess';
import { ChromeButton, Pill } from '@/shell/chrome';
import { CodeEditor } from '@/shell/codeEditor';
import { FormField, FormInput, FormSelect } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { PageBody } from '@/shell/PageHeader';

export type ValidationKind = 'pending' | 'checking' | 'valid' | 'invalid';

export interface ValidationState {
  kind: ValidationKind;
  message?: string | undefined;
}

export type RunState =
  | { kind: 'idle' }
  | { kind: 'running' }
  | { kind: 'success'; durationMs: number }
  | { kind: 'error'; durationMs?: number | undefined; message: string };

export function FunctionWorkbench({
  name,
  language,
  source,
  sampleInput,
  sampleOutput,
  validation,
  runState,
  validationMessage,
  outputPlaceholder,
  sampleInputError,
  writeAccess,
  runAccess,
  savePending,
  runPending,
  canRun,
  canSave,
  actions,
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
  validationMessage: string;
  outputPlaceholder: string;
  sampleInputError: string | null;
  writeAccess: ActionAccess;
  runAccess: ActionAccess;
  savePending: boolean;
  runPending: boolean;
  canRun: boolean;
  canSave: boolean;
  actions: React.ReactNode;
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

  return (
    <PageBody className="flex flex-col p-3 sm:p-4 xl:p-5">
      <form
        id="function-form"
        onSubmit={onSubmit}
        className="flex min-h-0 flex-1 flex-col gap-5"
      >
        <section
          data-function-definition
          aria-label={t('edit.definition')}
          className="pb-5"
        >
          <div className="grid grid-cols-1 gap-3 md:grid-cols-[minmax(0,1fr)_220px]">
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
          className="grid min-h-0 grid-cols-1 divide-y divide-bd-0 border-y border-bd-0 lg:grid-cols-[minmax(0,2.1fr)_minmax(320px,1fr)] lg:divide-x lg:divide-y-0"
        >
          <section className="flex min-w-0 flex-col overflow-hidden">
            <div className="flex min-h-12 flex-wrap items-center gap-2 border-b border-bd-0 px-1 py-2 sm:px-3 lg:min-h-16">
              <div className="flex min-w-0 flex-1 items-center gap-2.5">
                <Code2 className="h-4 w-4 shrink-0 text-indigo-soft" />
                <span className="truncate font-sans text-sm font-strong text-tx-0">
                  {t('edit.source_editor')}
                </span>
              </div>
              <div className="flex shrink-0 items-center gap-2">
                <Pill tone={language === 'vrl' ? 'yellow' : 'blue'}>
                  {language === 'vrl' ? 'VRL' : 'JavaScript'}
                </Pill>
                <span aria-live="polite">
                  <WorkbenchStatus kind={validation.kind}>
                    {t(`edit.validation.${validation.kind}`)}
                  </WorkbenchStatus>
                </span>
                <ChromeButton
                  variant="ghost"
                  size="sm"
                  disabled={writeAccess.disabled}
                  disabledReason={writeAccess.reason}
                  onClick={onFormatSource}
                >
                  <WandSparkles className="h-3.5 w-3.5" />
                  {t('edit.format')}
                </ChromeButton>
              </div>
            </div>
            {validation.kind === 'invalid' && (
              <div
                data-function-validation-detail
                role="status"
                className="flex items-start gap-2 border-b border-red/30 bg-red-dim px-3 py-2 font-sans text-xs leading-relaxed text-red-soft"
              >
                <StatusIcon kind="invalid" />
                <span className="min-w-0 break-words">{validationMessage}</span>
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
              className="rounded-none border-0 shadow-none"
            />
          </section>

          <aside
            aria-label={t('edit.test_runner')}
            className="flex min-w-0 flex-col overflow-hidden"
          >
            <div className="flex min-h-12 items-center gap-3 border-b border-bd-0 px-1 py-2 sm:px-3 lg:min-h-16">
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2 font-sans text-sm font-strong text-tx-0">
                  <Play className="h-4 w-4 text-indigo-soft" />
                  {t('edit.test_runner')}
                </div>
                <div className="mt-0.5 truncate font-sans text-xs text-tx-3">
                  {t('edit.test_runner_hint')}
                </div>
              </div>
              <ChromeButton
                size="sm"
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
            </div>

            <div className="flex min-h-9 items-center gap-2 border-b border-bd-0 bg-bg-2 px-3 py-1.5">
              <span className="font-mono text-xs font-strong text-tx-1">
                {t('edit.sample_input_step')}
              </span>
              <span
                className={cn(
                  'ml-auto inline-flex items-center gap-1.5 font-sans text-xs',
                  sampleInputError ? 'text-red-soft' : 'text-green-soft',
                )}
              >
                {sampleInputError ? (
                  <CircleAlert className="h-3.5 w-3.5" />
                ) : (
                  <CheckCircle2 className="h-3.5 w-3.5" />
                )}
                {sampleInputError
                  ? t('edit.json_invalid')
                  : t('edit.json_valid')}
              </span>
              <ChromeButton
                variant="ghost"
                size="sm"
                onClick={onFormatInput}
                disabled={runAccess.disabled || sampleInputError !== null}
                disabledReason={runAccess.reason}
                className="ml-1"
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
              className="rounded-none border-0 shadow-none"
            />

            <div className="flex min-h-9 items-center gap-2 border-y border-bd-0 bg-bg-2 px-3 py-1.5">
              <span className="font-mono text-xs font-strong text-tx-1">
                {t('edit.sample_output_step')}
              </span>
              <div className="ml-auto" aria-live="polite">
                <RunStatus state={runState} />
              </div>
            </div>
            {runState.kind === 'error' ? (
              <div className="min-h-[220px] bg-bg-0 p-4">
                <div className="flex items-start gap-2 border-l-2 border-red bg-red-dim p-3">
                  <XCircle className="mt-0.5 h-4 w-4 shrink-0 text-red-soft" />
                  <div className="min-w-0">
                    <div className="font-sans text-xs font-strong text-red-soft">
                      {t('edit.run_failed')}
                    </div>
                    <pre className="mt-2 whitespace-pre-wrap break-words font-mono text-xs leading-relaxed text-tx-1">
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
                className="rounded-none border-0 shadow-none"
              />
            )}
          </aside>
        </div>

        <div
          data-function-actions
          className="mt-auto flex flex-wrap items-center justify-end gap-2 pb-6 sm:pb-10"
        >
          {actions}
        </div>
      </form>
    </PageBody>
  );
}

function WorkbenchStatus({
  kind,
  children,
}: {
  kind: ValidationKind;
  children: React.ReactNode;
}) {
  return (
    <span
      className={cn(
        'inline-flex h-[22px] items-center gap-1.5 whitespace-nowrap rounded-full border px-2 font-sans text-xs font-strong',
        kind === 'valid' && 'border-green/30 bg-green-dim text-green-soft',
        kind === 'invalid' && 'border-red/30 bg-red-dim text-red-soft',
        kind === 'checking' && 'border-blue/30 bg-blue-dim text-blue-soft',
        kind === 'pending' && 'border-bd-0 bg-bg-2 text-tx-3',
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
