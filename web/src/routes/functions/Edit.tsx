import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  Loader2,
  Play,
  Save,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams } from 'react-router-dom';

import { ConfirmDialog } from '@/admin';
import * as functionsApi from '@/api/functions';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { PageHeader } from '@/shell/PageHeader';
import { QueryState, queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';

import {
  DEFAULT_VRL_SOURCE,
  defaultFunctionSource,
  normalizeLoadedFunctionSource,
} from './edit/defaults';
import {
  FunctionWorkbench,
  type RunState,
  type ValidationState,
} from './edit/Workbench';
import { formatFunctionSource, formatSampleInput, parseSampleInput } from './workbench';

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
  const [source, setSource] = React.useState(DEFAULT_VRL_SOURCE);
  const [sampleInput, setSampleInput] = React.useState('{\n  "level": "info",\n  "message": "hello"\n}');
  const [sampleOutput, setSampleOutput] = React.useState('');
  const [validation, setValidation] = React.useState<ValidationState>(INITIAL_VALIDATION);
  const [runState, setRunState] = React.useState<RunState>(INITIAL_RUN_STATE);
  const runStartedAt = React.useRef(0);

  React.useEffect(() => {
    if (!existing.data) return;
    setName(existing.data.name);
    setLanguage(existing.data.language);
    setSource(normalizeLoadedFunctionSource(existing.data.language, existing.data.source));
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
    setSource(defaultFunctionSource(nextLanguage));
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
      <FunctionWorkbench
        name={name}
        language={language}
        source={source}
        sampleInput={sampleInput}
        sampleOutput={sampleOutput}
        validation={validation}
        runState={runState}
        validationMessage={validationMessage}
        outputPlaceholder={outputPlaceholder}
        sampleInputError={sampleInputError}
        writeAccess={writeAccess}
        runAccess={runAccess}
        savePending={save.isPending}
        runPending={dryRun.isPending}
        canRun={canRun}
        canSave={canSave}
        actions={
          <>
            {!isNew && (
              <ChromeButton
                type="button"
                disabled={deleteAccess.disabled}
                disabledReason={deleteAccess.reason}
                onClick={() =>
                  deleteAccess.allowed && setConfirmDelete(true)
                }
                className="h-11 border-red text-red-soft enabled:hover:bg-red-dim sm:h-10"
              >
                {t('edit.delete')}
              </ChromeButton>
            )}
            <ChromeButton
              type="button"
              variant="ghost"
              onClick={() => navigate('/functions')}
              className="h-11 sm:h-10"
            >
              {t('edit.cancel')}
            </ChromeButton>
            <ChromeButton
              type="button"
              onClick={runSample}
              disabled={!canRun}
              disabledReason={runAccess.reason}
              className="h-11 sm:h-10"
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
              type="submit"
              disabled={!canSave}
              disabledReason={writeAccess.reason}
              className="h-11 sm:h-10"
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
        onSubmit={submit}
        onNameChange={setName}
        onLanguageChange={handleLanguageChange}
        onSourceChange={handleSourceChange}
        onSampleInputChange={handleSampleInputChange}
        onFormatSource={formatSource}
        onFormatInput={formatInput}
        onRun={runSample}
        onSave={() => save.mutate()}
      />
    </>
  );
}
