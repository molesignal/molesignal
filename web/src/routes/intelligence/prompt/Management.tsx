import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  Braces,
  Check,
  CopyPlus,
  FileText,
  Pencil,
  Plus,
  RotateCcw,
  Search,
  Star,
  Trash2,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as promptsApi from '@/api/intelligence/prompts';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { ProductState } from '@/product/states';
import {
  FormDrawer,
  FormField,
  FormRow,
  FormSelect,
  FormTextarea,
} from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { Badge } from '@/shell/ui/badge';
import { Button } from '@/shell/ui/button';
import { Input } from '@/shell/ui/input';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';

import {
  effectivePromptIds,
  groupPromptsByPurpose,
  parsePromptSchema,
  promptTemplateVariables,
  unknownPromptVariables,
} from './model';

type PromptEditorTarget =
  | { kind: 'create' }
  | { kind: 'view'; prompt: promptsApi.AgentPrompt }
  | { kind: 'edit'; prompt: promptsApi.AgentPrompt }
  | { kind: 'override'; prompt: promptsApi.AgentPrompt }
  | null;

type PromptRowAction =
  | { kind: 'set_default'; prompt: promptsApi.AgentPrompt }
  | { kind: 'restore'; prompt: promptsApi.AgentPrompt }
  | { kind: 'delete'; prompt: promptsApi.AgentPrompt };

const PURPOSES: promptsApi.PromptPurpose[] = [
  'system',
  'anomaly_analysis',
  'root_cause',
  'alert_explain',
  'query_generation',
];

export function PromptManagementPanel() {
  const { t } = useTranslation('intelligence');
  const queryClient = useQueryClient();
  const [editor, setEditor] = React.useState<PromptEditorTarget>(null);
  const [search, setSearch] = React.useState('');
  const prompts = useQuery({
    queryKey: ['intelligence', 'prompts'],
    queryFn: promptsApi.list,
    retry: false,
  });
  const rowAction = useMutation({
    mutationFn: async (action: PromptRowAction) => {
      if (action.kind === 'set_default') {
        return promptsApi.setDefault(action.prompt.id);
      }
      if (action.kind === 'restore') {
        return promptsApi.restore(action.prompt.id);
      }
      await promptsApi.remove(action.prompt.id);
      return null;
    },
    onSuccess: async (_result, action) => {
      await queryClient.invalidateQueries({
        queryKey: ['intelligence', 'prompts'],
      });
      toast.success(t(`settings.prompts.messages.${action.kind}`));
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  if (prompts.isLoading) return <ProductState variant="loading" />;
  if (prompts.isError) {
    return <ProductState variant="error" error={prompts.error} />;
  }

  const rows = prompts.data ?? [];
  const normalizedSearch = search.trim().toLocaleLowerCase();
  const filtered = normalizedSearch
    ? rows.filter((prompt) =>
        [
          prompt.name,
          prompt.body,
          prompt.purpose,
          prompt.scope,
          prompt.builtin_key ?? '',
        ].some((value) => value.toLocaleLowerCase().includes(normalizedSearch)),
      )
    : rows;
  const effectiveIds = effectivePromptIds(rows);
  const groups = groupPromptsByPurpose(filtered);
  const customized = rows.filter((prompt) => prompt.scope !== 'builtin').length;

  const runAction = (action: PromptRowAction) => {
    if (
      action.kind === 'delete' &&
      !window.confirm(
        t('settings.prompts.delete_confirm', { name: action.prompt.name }),
      )
    ) {
      return;
    }
    rowAction.mutate(action);
  };

  return (
    <>
      <section className="overflow-hidden rounded-lg border border-bd-0 bg-bg-1">
        <div className="flex flex-wrap items-start gap-4 border-b border-bd-0 px-4 py-3">
          <div className="min-w-0 flex-1">
            <h2 className="font-strong text-tx-0">
              {t('settings.prompts.title')}
            </h2>
            <p className="mt-1 max-w-3xl text-xs leading-5 text-tx-3">
              {t('settings.prompts.description')}
            </p>
          </div>
          <Button size="sm" onClick={() => setEditor({ kind: 'create' })}>
            <Plus /> {t('settings.prompts.create')}
          </Button>
        </div>

        <div className="grid gap-px border-b border-bd-0 bg-bd-0 sm:grid-cols-3">
          <PromptStat
            label={t('settings.prompts.stats.total')}
            value={rows.length}
            hint={t('settings.prompts.stats.total_hint')}
          />
          <PromptStat
            label={t('settings.prompts.stats.effective')}
            value={effectiveIds.size}
            hint={t('settings.prompts.stats.effective_hint')}
          />
          <PromptStat
            label={t('settings.prompts.stats.customized')}
            value={customized}
            hint={t('settings.prompts.stats.customized_hint')}
          />
        </div>

        <div className="border-b border-bd-0 p-3">
          <label className="relative block max-w-xl">
            <span className="sr-only">
              {t('settings.prompts.search_label')}
            </span>
            <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-tx-3" />
            <Input
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              placeholder={t('settings.prompts.search_placeholder')}
              className="bg-bg-2 pl-9 shadow-none"
            />
          </label>
        </div>

        {groups.length ? (
          <div className="divide-y divide-bd-0">
            {groups.map((group) => (
              <section key={group.purpose}>
                <div className="flex items-center gap-3 bg-bg-2 px-4 py-2.5">
                  <Braces className="h-4 w-4 text-tx-3" />
                  <div className="min-w-0 flex-1">
                    <h3 className="text-sm font-strong text-tx-1">
                      {t(`settings.prompts.purposes.${group.purpose}.title`)}
                    </h3>
                    <p className="mt-0.5 text-xs text-tx-3">
                      {t(
                        `settings.prompts.purposes.${group.purpose}.description`,
                      )}
                    </p>
                  </div>
                  <Badge variant="outline">{group.prompts.length}</Badge>
                </div>
                <div className="divide-y divide-bd-0">
                  {group.prompts.map((prompt) => (
                    <PromptRow
                      key={prompt.id}
                      prompt={prompt}
                      effective={effectiveIds.has(prompt.id)}
                      busy={
                        rowAction.isPending &&
                        rowAction.variables?.prompt.id === prompt.id
                      }
                      onView={() => setEditor({ kind: 'view', prompt })}
                      onEdit={() => setEditor({ kind: 'edit', prompt })}
                      onOverride={() =>
                        setEditor({ kind: 'override', prompt })
                      }
                      onAction={runAction}
                    />
                  ))}
                </div>
              </section>
            ))}
          </div>
        ) : (
          <ProductState
            variant="empty"
            compact
            title={t(
              normalizedSearch
                ? 'settings.prompts.empty_filter'
                : 'settings.prompts.empty_title',
            )}
            description={
              normalizedSearch
                ? t('settings.prompts.empty_filter_description')
                : t('settings.prompts.empty_description')
            }
          />
        )}
      </section>
      <PromptEditorDrawer target={editor} onClose={() => setEditor(null)} />
    </>
  );
}

function PromptStat({
  label,
  value,
  hint,
}: {
  label: string;
  value: number;
  hint: string;
}) {
  return (
    <div className="bg-bg-1 px-4 py-3">
      <div className="font-mono text-xl font-display-strong text-tx-0">
        {value}
      </div>
      <div className="mt-0.5 text-xs font-strong text-tx-1">{label}</div>
      <div className="mt-0.5 text-xs text-tx-3">{hint}</div>
    </div>
  );
}

function PromptRow({
  prompt,
  effective,
  busy,
  onView,
  onEdit,
  onOverride,
  onAction,
}: {
  prompt: promptsApi.AgentPrompt;
  effective: boolean;
  busy: boolean;
  onView: () => void;
  onEdit: () => void;
  onOverride: () => void;
  onAction: (action: PromptRowAction) => void;
}) {
  const { t } = useTranslation('intelligence');
  const builtin = prompt.scope === 'builtin';
  return (
    <article className="flex flex-col gap-3 px-4 py-3 lg:flex-row lg:items-center">
      <div
        className={cn(
          'grid h-9 w-9 shrink-0 place-items-center rounded-md border',
          effective
            ? 'border-indigo/30 bg-indigo/10 text-indigo'
            : 'border-bd-0 bg-bg-2 text-tx-3',
        )}
      >
        {effective ? (
          <Check className="h-4 w-4" />
        ) : (
          <FileText className="h-4 w-4" />
        )}
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-2">
          <h4 className="font-strong text-tx-0">{prompt.name}</h4>
          <Badge variant="outline">
            {t(`settings.prompts.scopes.${prompt.scope}`)}
          </Badge>
          {effective && (
            <Badge variant="accent">
              {t('settings.prompts.effective')}
            </Badge>
          )}
          {!prompt.enabled && (
            <Badge variant="secondary">{t('status.disabled')}</Badge>
          )}
        </div>
        <p className="mt-1 line-clamp-1 font-mono text-xs text-tx-3">
          {prompt.body}
        </p>
        <div className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-tx-3">
          <span>{t('settings.prompts.version', { version: prompt.version })}</span>
          <span>{formatMicrosActive(prompt.updated_at_micros)}</span>
          {prompt.builtin_key && (
            <span className="font-mono">{prompt.builtin_key}</span>
          )}
        </div>
      </div>
      <div className="flex shrink-0 flex-wrap items-center gap-1.5 lg:justify-end">
        <Button size="sm" variant="ghost" onClick={onView}>
          {t('settings.prompts.view')}
        </Button>
        {builtin ? (
          <Button size="sm" variant="outline" onClick={onOverride}>
            <CopyPlus /> {t('settings.prompts.override')}
          </Button>
        ) : (
          <>
            <Button size="sm" variant="outline" onClick={onEdit}>
              <Pencil /> {t('common.edit')}
            </Button>
            {!prompt.is_default && (
              <Button
                size="sm"
                variant="ghost"
                disabled={busy}
                onClick={() => onAction({ kind: 'set_default', prompt })}
              >
                <Star /> {t('settings.prompts.set_default')}
              </Button>
            )}
            {prompt.builtin_key && (
              <Button
                size="sm"
                variant="ghost"
                disabled={busy}
                onClick={() => onAction({ kind: 'restore', prompt })}
              >
                <RotateCcw /> {t('settings.prompts.restore')}
              </Button>
            )}
            <Button
              size="sm"
              variant="ghost"
              disabled={busy}
              onClick={() => onAction({ kind: 'delete', prompt })}
              className="text-red hover:text-red"
            >
              <Trash2 /> {t('settings.prompts.delete')}
            </Button>
          </>
        )}
      </div>
    </article>
  );
}

interface PromptDraft {
  scope: 'org' | 'user';
  purpose: promptsApi.PromptPurpose;
  name: string;
  body: string;
  schemaSource: string;
  enabled: boolean;
  makeDefault: boolean;
}

function draftFor(target: Exclude<PromptEditorTarget, null>): PromptDraft {
  if (target.kind === 'create') {
    return {
      scope: 'org',
      purpose: 'system',
      name: '',
      body: '',
      schemaSource: JSON.stringify(
        { type: 'object', properties: {} },
        null,
        2,
      ),
      enabled: true,
      makeDefault: false,
    };
  }
  const source = target.prompt;
  return {
    scope: target.kind === 'override' ? 'org' : source.scope === 'user' ? 'user' : 'org',
    purpose: source.purpose,
    name:
      target.kind === 'override'
        ? `${source.name} · ${source.scope === 'builtin' ? 'override' : 'copy'}`
        : source.name,
    body: source.body,
    schemaSource: JSON.stringify(source.variables_schema, null, 2),
    enabled: source.enabled,
    makeDefault: target.kind === 'override' ? true : source.is_default,
  };
}

function PromptEditorDrawer({
  target,
  onClose,
}: {
  target: PromptEditorTarget;
  onClose: () => void;
}) {
  const { t } = useTranslation('intelligence');
  const queryClient = useQueryClient();
  const [draft, setDraft] = React.useState<PromptDraft | null>(null);
  const [formError, setFormError] = React.useState('');

  React.useEffect(() => {
    setDraft(target ? draftFor(target) : null);
    setFormError('');
  }, [target]);

  const save = useMutation({
    mutationFn: async () => {
      if (!target || !draft) return null;
      const name = draft.name.trim();
      const body = draft.body.trim();
      if (!name || !body) {
        throw new Error(t('settings.prompts.validation.required'));
      }
      const parsed = parsePromptSchema(draft.schemaSource);
      if (!parsed.ok) {
        throw new Error(
          parsed.message === 'schema_object_required' ||
            parsed.message === 'schema_properties_required'
            ? t(`settings.prompts.validation.${parsed.message}`)
            : t('settings.prompts.validation.invalid_schema', {
                message: parsed.message,
              }),
        );
      }
      const unknown = unknownPromptVariables(body, parsed.variables);
      if (unknown.length > 0) {
        throw new Error(
          t('settings.prompts.validation.unknown_variables', {
            variables: unknown.join(', '),
          }),
        );
      }

      let saved: promptsApi.AgentPrompt;
      if (target.kind === 'edit') {
        saved = await promptsApi.update(target.prompt.id, {
          name,
          body,
          variables_schema: parsed.schema,
          enabled: draft.enabled,
        });
      } else {
        const source =
          target.kind === 'override' ? target.prompt : undefined;
        saved = await promptsApi.create({
          scope: draft.scope,
          purpose: draft.purpose,
          name,
          body,
          variables_schema: parsed.schema,
          enabled: draft.enabled,
          ...(source?.builtin_key
            ? { builtin_key: source.builtin_key }
            : {}),
          ...(source ? { parent_id: source.id } : {}),
        });
      }
      if (draft.makeDefault && !saved.is_default) {
        saved = await promptsApi.setDefault(saved.id);
      }
      return saved;
    },
    onSuccess: async (saved) => {
      if (!saved) return;
      await queryClient.invalidateQueries({
        queryKey: ['intelligence', 'prompts'],
      });
      toast.success(
        t(
          target?.kind === 'edit'
            ? 'settings.prompts.messages.updated'
            : 'settings.prompts.messages.created',
        ),
      );
      onClose();
    },
    onError: (error) => setFormError(toApiError(error).message),
  });

  if (!target || !draft) return null;
  const readOnly = target.kind === 'view';
  const source = target.kind === 'create' ? null : target.prompt;
  const schema = parsePromptSchema(draft.schemaSource);
  const declaredVariables = schema.ok ? schema.variables : [];
  const referencedVariables = promptTemplateVariables(draft.body);
  const unknownVariables = schema.ok
    ? unknownPromptVariables(draft.body, declaredVariables)
    : [];
  const title = readOnly
    ? t('settings.prompts.editor.view_title')
    : target.kind === 'edit'
      ? t('settings.prompts.editor.edit_title')
      : target.kind === 'override'
        ? t('settings.prompts.editor.override_title')
        : t('settings.prompts.editor.create_title');

  return (
    <FormDrawer
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      title={title}
      subtitle={t('settings.prompts.editor.description')}
      width={820}
      footer={
        readOnly ? (
          <Button variant="outline" onClick={onClose}>
            {t('common.close')}
          </Button>
        ) : (
          <>
            <Button variant="outline" onClick={onClose}>
              {t('common.cancel')}
            </Button>
            <Button
              onClick={() => save.mutate()}
              disabled={save.isPending}
            >
              {save.isPending ? t('common.saving') : t('common.save')}
            </Button>
          </>
        )
      }
    >
      <div className="flex flex-col gap-6">
        {source && (
          <div className="rounded-md border border-bd-0 bg-bg-2 px-3 py-2.5 text-xs text-tx-2">
            <span className="font-strong text-tx-1">
              {t('settings.prompts.editor.source')}:
            </span>{' '}
            {source.name} · {t(`settings.prompts.scopes.${source.scope}`)} ·{' '}
            {t('settings.prompts.version', { version: source.version })}
          </div>
        )}

        <FormRow className="max-sm:grid-cols-1">
          <FormField label={t('settings.prompts.fields.scope')} required>
            <FormSelect
              value={draft.scope}
              onChange={(scope) =>
                setDraft((current) =>
                  current
                    ? { ...current, scope: scope as 'org' | 'user' }
                    : current,
                )
              }
              disabled={readOnly || target.kind === 'edit'}
              options={[
                {
                  value: 'org',
                  label: t('settings.prompts.scopes.org'),
                },
                {
                  value: 'user',
                  label: t('settings.prompts.scopes.user'),
                },
              ]}
            />
          </FormField>
          <FormField label={t('settings.prompts.fields.purpose')} required>
            <FormSelect
              value={draft.purpose}
              onChange={(purpose) =>
                setDraft((current) =>
                  current
                    ? {
                        ...current,
                        purpose: purpose as promptsApi.PromptPurpose,
                      }
                    : current,
                )
              }
              disabled={
                readOnly ||
                target.kind === 'edit' ||
                target.kind === 'override'
              }
              options={PURPOSES.map((purpose) => ({
                value: purpose,
                label: t(`settings.prompts.purposes.${purpose}.title`),
              }))}
            />
          </FormField>
        </FormRow>

        <FormField label={t('settings.prompts.fields.name')} required>
          <input
            value={draft.name}
            onChange={(event) =>
              setDraft((current) =>
                current ? { ...current, name: event.target.value } : current,
              )
            }
            readOnly={readOnly}
            placeholder={t('settings.prompts.name_placeholder')}
            className="h-9 w-full rounded-md border border-bd-1 bg-bg-2 px-3 text-sm text-tx-0 placeholder:text-tx-3 focus:outline-none read-only:cursor-default read-only:opacity-75"
          />
        </FormField>

        <FormField
          label={t('settings.prompts.fields.body')}
          hint={t('settings.prompts.body_hint')}
          required
        >
          <FormTextarea
            value={draft.body}
            onChange={(event) =>
              setDraft((current) =>
                current ? { ...current, body: event.target.value } : current,
              )
            }
            readOnly={readOnly}
            spellCheck={false}
            className="min-h-72 resize-y font-mono text-xs leading-6"
          />
        </FormField>

        <div>
          <div className="text-xs font-strong text-tx-1">
            {t('settings.prompts.variables.title')}
          </div>
          <p className="mt-1 text-xs leading-5 text-tx-3">
            {t('settings.prompts.variables.description')}
          </p>
          <div className="mt-2 flex min-h-8 flex-wrap items-center gap-1.5">
            {referencedVariables.length ? (
              referencedVariables.map((variable) => (
                <Badge
                  key={variable}
                  variant="outline"
                  className={cn(
                    'font-mono',
                    unknownVariables.includes(variable) &&
                      'border-red/40 text-red',
                  )}
                >
                  {`{{ ${variable} }}`}
                </Badge>
              ))
            ) : (
              <span className="text-xs text-tx-3">
                {t('settings.prompts.variables.none')}
              </span>
            )}
          </div>
        </div>

        <FormField
          label={t('settings.prompts.fields.schema')}
          hint={t('settings.prompts.schema_hint')}
          required
        >
          <FormTextarea
            value={draft.schemaSource}
            onChange={(event) =>
              setDraft((current) =>
                current
                  ? { ...current, schemaSource: event.target.value }
                  : current,
              )
            }
            readOnly={readOnly}
            spellCheck={false}
            className="min-h-44 resize-y font-mono text-xs leading-5"
          />
        </FormField>

        {!readOnly && (
          <div className="divide-y divide-bd-0 overflow-hidden rounded-md border border-bd-0">
            <PromptSwitchRow
              label={t('settings.prompts.fields.enabled')}
              description={t('settings.prompts.enabled_hint')}
              checked={draft.enabled}
              onCheckedChange={(enabled) =>
                setDraft((current) =>
                  current ? { ...current, enabled } : current,
                )
              }
            />
            <PromptSwitchRow
              label={t('settings.prompts.fields.default')}
              description={t('settings.prompts.default_hint')}
              checked={draft.makeDefault}
              onCheckedChange={(makeDefault) =>
                setDraft((current) =>
                  current ? { ...current, makeDefault } : current,
                )
              }
            />
          </div>
        )}

        {formError && (
          <div
            role="alert"
            className="rounded-md border border-red/30 bg-red/5 px-3 py-2.5 text-sm text-red"
          >
            {formError}
          </div>
        )}
      </div>
    </FormDrawer>
  );
}

function PromptSwitchRow({
  label,
  description,
  checked,
  onCheckedChange,
}: {
  label: string;
  description: string;
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
}) {
  return (
    <label className="flex min-h-14 cursor-pointer items-center gap-4 bg-bg-2 px-3 py-2.5">
      <span className="min-w-0 flex-1">
        <span className="block text-sm font-strong text-tx-1">{label}</span>
        <span className="mt-0.5 block text-xs leading-5 text-tx-3">
          {description}
        </span>
      </span>
      <Switch checked={checked} onCheckedChange={onCheckedChange} />
    </label>
  );
}
