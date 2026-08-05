import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  CheckCircle2,
  CircleAlert,
  CircleDotDashed,
  Code2,
  Loader2,
  Play,
  Save,
  WandSparkles,
  XCircle,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams } from 'react-router-dom';

import { ConfirmDialog } from '@/admin';
import * as functionsApi from '@/api/functions';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton, Pill } from '@/shell/chrome';
import { CodeEditor } from '@/shell/codeEditor';
import { FormField, FormInput, FormSelect } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { PageBody, PageHeader } from '@/shell/PageHeader';
import { QueryState, queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';

import { formatFunctionSource, formatSampleInput, parseSampleInput } from './workbench';

const DEFAULT_VRL = `# VRL transform — receives event in \`.\`, returns the modified event.
.environment = "production"
.timestamp = now()`;

const DEFAULT_JS = `// JS transform — receives event, returns the modified event.
export default function transform(event) {
  return { ...event, environment: 'production' };
}`;

type ValidationKind = 'pending' | 'checking' | 'valid' | 'invalid';

interface ValidationState {
  kind: ValidationKind;
  message?: string;
}

type RunState =
  | { kind: 'idle' }
  | { kind: 'running' }
  | { kind: 'success'; durationMs: number }
  | { kind: 'error'; durationMs?: number; message: string };

const INITIAL_VALIDATION: ValidationState = { kind: 'pending' };
const INITIAL_RUN_STATE: RunState = { kind: 'idle' };

export function FunctionsEdit() {
  const { t } = useTranslation('functions');
  const { id = 'new' } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const qc = useQueryClient();
  const isNew = id === 'new';
  const writeAccess = useActionAccess({
    permission: isNew ? 'functions.create' : 'functions.edit',
  });
  const runAccess = useActionAccess({
    permission: 'functions.run',
  });
  const deleteAccess = useActionAccess({
    permission: 'functions.delete',
  });
  const [confirmDelete, setConfirmDelete] = React.useState(false);

  const existing = useQuery({
    queryKey: ['functions', 'get', id],
    queryFn: () => functionsApi.get(id),
    enabled: !isNew,
  });

  const [name, setName] = React.useState('');
  const [language, setLanguage] = React.useState<functionsApi.FunctionLanguage>('vrl');
  const [source, setSource] = React.useState(DEFAULT_VRL);
  const [sampleInput, setSampleInput] = React.useState('{\n  "level": "info",\n  "message": "hello"\n}');
  const [sampleOutput, setSampleOutput] = React.useState('');
  const [validation, setValidation] = React.useState<ValidationState>(INITIAL_VALIDATION);
  const [runState, setRunState] = React.useState<RunState>(INITIAL_RUN_STATE);
  const runStartedAt = React.useRef(0);

  React.useEffect(() => {
    if (!existing.data) return;
    setName(existing.data.name);
    setLanguage(existing.data.language);
    setSource(existing.data.source);
    setValidation(INITIAL_VALIDATION);
    setRunState(INITIAL_RUN_STATE);
    setSampleOutput('');
  }, [existing.data]);

  const save = useMutation({
    mutationFn: () => {
      const payload: functionsApi.FunctionInput = {
        name: name.trim(),
        language,
        source,
      };
      return isNew ? functionsApi.create(payload) : functionsApi.update(id, payload);
    },
    onMutate: () => {
      setValidation({ kind: 'checking' });
    },
    onSuccess: (resp) => {
      setValidation({ kind: 'valid' });
      toast.success(t('edit.toast_saved'));
      void qc.invalidateQueries({ queryKey: ['functions', 'list'] });
      if (isNew) navigate(`/functions/${encodeURIComponent(resp.id)}`);
    },
    onError: (err) => {
      const message = toApiError(err).message;
      setValidation({ kind: 'invalid', message });
      toast.error(message);
    },
  });

  const remove = useMutation({
    mutationFn: () => functionsApi.remove(id),
    onSuccess: () => {
      toast.success(t('edit.toast_deleted'));
      void qc.invalidateQueries({ queryKey: ['functions', 'list'] });
      navigate('/functions');
    },
  });

  const dryRun = useMutation({
    mutationFn: (input: unknown) => functionsApi.run({ language, source, input }),
    onMutate: () => {
      runStartedAt.current = Date.now();
      setRunState({ kind: 'running' });
      setValidation({ kind: 'checking' });
      setSampleOutput('');
    },
    onSuccess: (resp) => {
      const durationMs = Math.max(1, Date.now() - runStartedAt.current);
      setSampleOutput(JSON.stringify(resp.output, null, 2));
      setRunState({ kind: 'success', durationMs });
      setValidation({ kind: 'valid' });
      toast.success(t('edit.run_complete'));
    },
    onError: (err) => {
      const message = toApiError(err).message;
      const durationMs = Math.max(1, Date.now() - runStartedAt.current);
      setRunState({ kind: 'error', durationMs, message });
      setValidation({ kind: 'invalid', message });
      toast.error(message);
    },
  });

  const sampleInputError = React.useMemo(() => {
    try {
      parseSampleInput(sampleInput);
      return null;
    } catch {
      return t('edit.sample_input_invalid');
    }
  }, [sampleInput, t]);

  const editorLanguage = language === 'vrl' ? 'vrl' : 'javascript';
  const canRun =
    runAccess.allowed &&
    source.trim().length > 0 &&
    sampleInputError === null &&
    !dryRun.isPending;
  const canSave =
    writeAccess.allowed &&
    name.trim().length > 0 &&
    source.trim().length > 0 &&
    !save.isPending;

  const resetExecution = React.useCallback(() => {
    setValidation(INITIAL_VALIDATION);
    setRunState(INITIAL_RUN_STATE);
    setSampleOutput('');
  }, []);

  const handleSourceChange = React.useCallback((next: string) => {
    setSource(next);
    resetExecution();
  }, [resetExecution]);

  const handleSampleInputChange = React.useCallback((next: string) => {
    setSampleInput(next);
    setRunState(INITIAL_RUN_STATE);
    setSampleOutput('');
  }, []);

  const handleLanguageChange = React.useCallback((next: string) => {
    const nextLanguage = next as functionsApi.FunctionLanguage;
    if (nextLanguage === language) return;
    setLanguage(nextLanguage);
    setSource(nextLanguage === 'vrl' ? DEFAULT_VRL : DEFAULT_JS);
    resetExecution();
  }, [language, resetExecution]);

  const runSample = React.useCallback(() => {
    if (!runAccess.allowed) return;
    try {
      dryRun.mutate(parseSampleInput(sampleInput));
    } catch {
      const message = t('edit.sample_input_invalid');
      setRunState({ kind: 'error', message });
      toast.error(message);
    }
  }, [dryRun, runAccess.allowed, sampleInput, t]);

  const formatSource = React.useCallback(() => {
    const next = formatFunctionSource(source);
    setSource(next);
    resetExecution();
    toast.success(t('edit.format_complete'));
  }, [resetExecution, source, t]);

  const formatInput = React.useCallback(() => {
    try {
      setSampleInput(formatSampleInput(sampleInput));
      setRunState(INITIAL_RUN_STATE);
      setSampleOutput('');
      toast.success(t('edit.json_formatted'));
    } catch {
      const message = t('edit.sample_input_invalid');
      setRunState({ kind: 'error', message });
      toast.error(message);
    }
  }, [sampleInput, t]);

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (canSave) save.mutate();
  };

  if (!isNew) {
    const state = queryStateFor({
      isLoading: existing.isLoading,
      isError: existing.isError,
      data: existing.data,
    });
    if (state) {
      return (
        <>
          <PageHeader
            breadcrumbs={[{ labelKey: 'functions', label: t('title'), to: '/functions' }]}
            title={t('edit.edit_title')}
          />
          <div className="p-4">
            <QueryState state={state} error={existing.error} emptyLabel={t('edit.not_found')} />
          </div>
        </>
      );
    }
  }

  const validationMessage =
    validation.message
    ?? (validation.kind === 'valid'
      ? t('edit.validation.valid_detail')
      : validation.kind === 'checking'
        ? t('edit.validation.checking_detail')
        : validation.kind === 'invalid'
          ? t('edit.validation.invalid_detail')
          : t('edit.validation.pending_detail'));

  const outputPlaceholder =
    runState.kind === 'running'
      ? t('edit.run_output_running')
      : t('edit.run_output_placeholder');

  return (
    <>
      <PageHeader
        breadcrumbs={[
          { labelKey: 'functions', label: t('title'), to: '/functions' },
          { labelKey: 'edit', label: isNew ? t('edit.create_title') : name || t('edit.edit_title') },
        ]}
        title={isNew ? t('edit.create_title') : `${t('edit.edit_title')} · ${name}`}
        subtitle={t('edit.workspace_subtitle')}
        toolbar={
          <>
            {!isNew && (
              <ChromeButton
                disabled={deleteAccess.disabled}
                disabledReason={deleteAccess.reason}
                onClick={() =>
                  deleteAccess.allowed && setConfirmDelete(true)
                }
                className="border-red text-red-soft enabled:hover:bg-red-dim"
              >
                {t('edit.delete')}
              </ChromeButton>
            )}
            <ChromeButton variant="ghost" onClick={() => navigate('/functions')}>
              {t('edit.cancel')}
            </ChromeButton>
            <ChromeButton
              onClick={runSample}
              disabled={!canRun}
              disabledReason={runAccess.reason}
            >
              {dryRun.isPending ? (
                <Loader2 className="h-4 w-4 animate-spin motion-reduce:animate-none" />
              ) : (
                <Play className="h-4 w-4" />
              )}
              {dryRun.isPending ? t('edit.running') : t('edit.run_test')}
            </ChromeButton>
            <ChromeButton
              variant="primary"
              form="function-form"
              type="submit"
              disabled={!canSave}
              disabledReason={writeAccess.reason}
            >
              {save.isPending ? (
                <Loader2 className="h-4 w-4 animate-spin motion-reduce:animate-none" />
              ) : (
                <Save className="h-4 w-4" />
              )}
              {save.isPending ? t('edit.saving') : t('edit.save')}
            </ChromeButton>
          </>
        }
      />
      <ConfirmDialog
        open={confirmDelete}
        onOpenChange={setConfirmDelete}
        destructive
        title={t('edit.delete_confirm_title')}
        description={t('edit.delete_confirm_description')}
        confirmLabel={t('edit.delete_confirm')}
        busy={remove.isPending}
        disabled={deleteAccess.disabled}
        disabledReason={deleteAccess.reason}
        onConfirm={() => deleteAccess.allowed && remove.mutate()}
      />
      <PageBody className="flex flex-col p-3 sm:p-4 xl:p-5">
        <form id="function-form" onSubmit={submit} className="flex min-h-0 flex-1 flex-col gap-3">
          <section
            aria-label={t('edit.definition')}
            className="rounded-lg border border-bd-0 bg-bg-1 px-4 py-3"
          >
            <div className="grid grid-cols-1 gap-3 md:grid-cols-[minmax(0,1fr)_220px]">
              <FormField label={t('edit.name_label')} required>
                <FormInput
                  value={name}
                  disabled={writeAccess.disabled || save.isPending}
                  disabledReason={writeAccess.reason}
                  onChange={(event) => setName(event.target.value)}
                  placeholder={t('edit.name_placeholder')}
                  autoComplete="off"
                  required
                />
              </FormField>
              <FormField label={t('edit.language_label')} required>
                <FormSelect
                  value={language}
                  disabled={writeAccess.disabled || save.isPending}
                  disabledReason={writeAccess.reason}
                  onChange={handleLanguageChange}
                  options={[
                    { value: 'vrl', label: 'VRL' },
                    { value: 'js', label: t('edit.languages.javascript') },
                  ]}
                />
              </FormField>
            </div>
          </section>

          <div className="grid min-h-0 flex-1 grid-cols-1 items-start gap-3 lg:grid-cols-[minmax(0,2.1fr)_minmax(320px,1fr)]">
            <section className="min-w-0 overflow-hidden rounded-lg border border-bd-1 bg-bg-1">
              <div className="flex min-h-12 flex-wrap items-center gap-2 border-b border-bd-0 px-4 py-2">
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
                  <WorkbenchStatus kind={validation.kind}>
                    {t(`edit.validation.${validation.kind}`)}
                  </WorkbenchStatus>
                  <ChromeButton
                    variant="ghost"
                    size="sm"
                    disabled={writeAccess.disabled}
                    disabledReason={writeAccess.reason}
                    onClick={formatSource}
                  >
                    <WandSparkles className="h-3.5 w-3.5" />
                    {t('edit.format')}
                  </ChromeButton>
                </div>
              </div>
              <CodeEditor
                value={source}
                onChange={handleSourceChange}
                readOnly={writeAccess.disabled}
                language={editorLanguage}
                ariaLabel={t('edit.source_label')}
                minHeight={460}
                maxHeight={720}
                onModEnter={runSample}
                onModSave={() => {
                  if (writeAccess.allowed && canSave) save.mutate();
                }}
                resizable
                showHeader={false}
                className="rounded-none border-0 shadow-none"
              />
              <div
                aria-live="polite"
                className="flex min-h-10 flex-wrap items-center gap-2 border-t border-bd-0 bg-bg-1 px-3 py-2 font-sans text-xs text-tx-2"
              >
                <StatusIcon kind={validation.kind} />
                <span className="min-w-0 flex-1">{validationMessage}</span>
                <div className="ml-auto hidden items-center gap-3 text-tx-3 sm:flex">
                  <span><KeyHint>{t('edit.shortcut_run_key')}</KeyHint> {t('edit.shortcut_run')}</span>
                  <span><KeyHint>{t('edit.shortcut_save_key')}</KeyHint> {t('edit.shortcut_save')}</span>
                </div>
              </div>
            </section>

            <aside
              aria-label={t('edit.test_runner')}
              className="min-w-0 overflow-hidden rounded-lg border border-bd-1 bg-bg-1"
            >
              <div className="flex min-h-12 items-center gap-3 border-b border-bd-0 px-4 py-2">
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
                  onClick={runSample}
                  disabled={!canRun}
                  disabledReason={runAccess.reason}
                >
                  {dryRun.isPending ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin motion-reduce:animate-none" />
                  ) : (
                    <Play className="h-3.5 w-3.5" />
                  )}
                  {dryRun.isPending ? t('edit.running') : t('edit.run_sample')}
                </ChromeButton>
              </div>

              <div className="flex min-h-9 items-center gap-2 border-b border-bd-0 bg-bg-2 px-3 py-1.5">
                <span className="font-mono text-xs font-strong text-tx-1">
                  {t('edit.sample_input_step')}
                </span>
                <span className={cn(
                  'ml-auto inline-flex items-center gap-1.5 font-sans text-xs',
                  sampleInputError ? 'text-red-soft' : 'text-green-soft',
                )}>
                  {sampleInputError ? <CircleAlert className="h-3.5 w-3.5" /> : <CheckCircle2 className="h-3.5 w-3.5" />}
                  {sampleInputError ? t('edit.json_invalid') : t('edit.json_valid')}
                </span>
                <ChromeButton
                  variant="ghost"
                  size="sm"
                  onClick={formatInput}
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
                onChange={handleSampleInputChange}
                readOnly={runAccess.disabled}
                language="json"
                ariaLabel={t('edit.sample_input')}
                minHeight={180}
                maxHeight={280}
                onModEnter={runSample}
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
                  <div className="flex items-start gap-2 rounded-md border border-red/30 bg-red-dim p-3">
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
              <div className="border-t border-bd-0 px-3 py-2 font-sans text-xs leading-relaxed text-tx-3">
                {t('edit.test_runner_footer')}
              </div>
            </aside>
          </div>
        </form>
      </PageBody>
    </>
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
  if (kind === 'valid') return <CheckCircle2 className="h-3.5 w-3.5 shrink-0" />;
  if (kind === 'invalid') return <XCircle className="h-3.5 w-3.5 shrink-0" />;
  if (kind === 'checking') {
    return <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin motion-reduce:animate-none" />;
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
    return <WorkbenchStatus kind="checking">{t('edit.run_running')}</WorkbenchStatus>;
  }
  return <WorkbenchStatus kind="pending">{t('edit.run_idle')}</WorkbenchStatus>;
}

function KeyHint({ children }: { children: React.ReactNode }) {
  return (
    <kbd className="rounded border border-bd-1 bg-bg-2 px-1.5 py-0.5 font-mono text-tx-2">
      {children}
    </kbd>
  );
}
