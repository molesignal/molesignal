import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { ExternalLink, Pencil, Pin, PinOff, Plus, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { ConfirmDialog, DataTable } from '@/admin';
import * as savedViewsApi from '@/api/savedViews';
import type { QueryLanguage, SavedView, SavedViewInput } from '@/api/savedViews';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import {
  type ActionAccess,
  restrictActionAccess,
  useActionAccess,
} from '@/product/actionAccess';
import { type ProductStateProps } from '@/product/states';
import { ListPage } from '@/product/templates';
import { ChromeButton, IconButton, Pill, type PillTone } from '@/shell/chrome';
import { CodeEditor } from '@/shell/codeEditor';
import {
  FormChecklist,
  FormDrawer,
  FormField,
  FormInput,
  FormRow,
  FormSection,
  FormSelect,
  FormSubmitFooter,
} from '@/shell/FormDrawer';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';

const LANGUAGE_TONE: Record<QueryLanguage, PillTone> = {
  sql: 'blue',
  promql: 'indigo',
};

/** Duration presets for the look-back window, in seconds. */
const TIME_RANGES: Array<{ value: string; label: string }> = [
  { value: '900', label: '15m' },
  { value: '3600', label: '1h' },
  { value: '21600', label: '6h' },
  { value: '86400', label: '24h' },
  { value: '604800', label: '7d' },
];

function formatRange(secs: number): string {
  const preset = TIME_RANGES.find((r) => r.value === String(secs));
  if (preset) return preset.label;
  if (secs % 86400 === 0) return `${secs / 86400}d`;
  if (secs % 3600 === 0) return `${secs / 3600}h`;
  if (secs % 60 === 0) return `${secs / 60}m`;
  return `${secs}s`;
}

/** Build the create/update payload from an existing view (pin toggle reuses it). */
function viewToInput(v: SavedView, overrides: Partial<SavedViewInput> = {}): SavedViewInput {
  return {
    name: v.name,
    language: v.language,
    statement: v.statement,
    time_range_secs: v.time_range_secs,
    stream: v.stream,
    tags: v.tags,
    pinned: v.pinned,
    ...overrides,
  };
}

export function SavedViews() {
  const { t } = useTranslation('saved-views');
  const qc = useQueryClient();
  const navigate = useNavigate();
  const createAccess = useActionAccess({
    permission: 'saved_views.create',
  });
  const editAccess = useActionAccess({
    permission: 'saved_views.edit',
  });
  const deleteAccess = useActionAccess({
    permission: 'saved_views.delete',
  });
  const [editing, setEditing] = React.useState<SavedView | 'new' | null>(null);
  const [confirmingDelete, setConfirmingDelete] = React.useState<SavedView | null>(null);

  const q = useQuery({
    queryKey: ['saved-views'],
    queryFn: () => savedViewsApi.list(),
  });
  const views = q.data ?? [];

  const invalidate = () => qc.invalidateQueries({ queryKey: ['saved-views'] });

  const pinMut = useMutation({
    mutationFn: (v: SavedView) => savedViewsApi.update(v.id, viewToInput(v, { pinned: !v.pinned })),
    onSuccess: () => void invalidate(),
    onError: (err: unknown) => toast.error(toApiError(err).message),
  });

  const deleteMut = useMutation({
    mutationFn: (id: string) => savedViewsApi.remove(id),
    onSuccess: () => {
      toast.success(t('toast.deleted', { defaultValue: 'Saved view deleted' }));
      setConfirmingDelete(null);
      void invalidate();
    },
    onError: (err: unknown) => toast.error(toApiError(err).message),
  });

  /** Open a saved view in the matching explorer with its look-back window. */
  const openView = (v: SavedView) => {
    const to = new Date();
    const from = new Date(to.getTime() - v.time_range_secs * 1000);
    // `?time=<from>..<to>` is the platform-wide window contract hydrated by
    // ShellRoot → hydrateFromSearchParams (absolute when neither side is `now`).
    const params = new URLSearchParams({ time: `${from.toISOString()}..${to.toISOString()}` });
    if (v.language === 'promql') {
      // Metrics seeds its PromQL editor verbatim from `?promql=`.
      params.set('promql', v.statement);
      navigate(`/metrics?${params.toString()}`);
    } else {
      // Logs reads `?q=` as a field-query DSL — a raw SQL statement must go via
      // `?sql=`, which drops Logs into SQL mode. `?stream=` selects the table.
      params.set('sql', v.statement);
      if (v.stream) params.set('stream', v.stream);
      navigate(`/logs?${params.toString()}`);
    }
  };

  const tableState = queryStateFor({ isLoading: q.isLoading, isError: q.isError, data: views });
  const listState: ProductStateProps | null =
    tableState === 'loading'
      ? { variant: 'loading' }
      : tableState === 'error'
        ? { variant: 'error', error: q.error }
        : tableState === 'empty'
          ? {
              variant: 'empty',
              title: t('states.empty_title', { defaultValue: 'No saved views yet' }),
              description: t('states.empty_description', {
                defaultValue: 'Save a SQL or PromQL query to reuse it across investigations.',
              }),
              action: (
                <ChromeButton
                  variant="primary"
                  disabled={createAccess.disabled}
                  disabledReason={createAccess.reason}
                  onClick={() => createAccess.allowed && setEditing('new')}
                >
                  <Plus className="h-3 w-3" /> {t('actions.new', { defaultValue: 'New saved view' })}
                </ChromeButton>
              ),
            }
          : null;

  return (
    <>
      <ListPage
        title={t('title', { defaultValue: 'Saved views' })}
        subtitle={t('subtitle', { defaultValue: 'Reusable SQL / PromQL queries, shared across your org.' })}
        toolbar={
          <ChromeButton
            variant="primary"
            disabled={createAccess.disabled}
            disabledReason={createAccess.reason}
            onClick={() => createAccess.allowed && setEditing('new')}
          >
            <Plus className="h-3 w-3" />
            {t('actions.new', { defaultValue: 'New saved view' })}
          </ChromeButton>
        }
        kpis={[
          { label: t('kpis.total', { defaultValue: 'Saved views' }), value: String(views.length) },
          {
            label: t('kpis.pinned', { defaultValue: 'Pinned' }),
            value: String(views.filter((v) => v.pinned).length),
          },
        ]}
        kpiLayout="inline"
        state={listState}
      >
        <DataTable
          rows={views}
          rowKey={(v) => v.id}
          columns={[
            {
              key: 'name',
              header: t('list.columns.name', { defaultValue: 'Name' }),
              cell: (v) => (
                <span className="flex items-center gap-1.5 text-tx-0">
                  {v.pinned && <Pin className="h-3 w-3 text-yellow" aria-hidden />}
                  {v.name}
                </span>
              ),
            },
            {
              key: 'language',
              header: t('list.columns.language', { defaultValue: 'Language' }),
              cell: (v) => <Pill tone={LANGUAGE_TONE[v.language]}>{v.language.toUpperCase()}</Pill>,
              width: 110,
            },
            {
              key: 'stream',
              header: t('list.columns.stream', { defaultValue: 'Stream' }),
              cell: (v) => <span className="text-tx-1">{v.stream || '—'}</span>,
              width: 160,
            },
            {
              key: 'tags',
              header: t('list.columns.tags', { defaultValue: 'Tags' }),
              cell: (v) =>
                v.tags.length > 0 ? (
                  <span className="flex flex-wrap gap-1">
                    {v.tags.map((tag) => (
                      <Pill key={tag} tone="dim">
                        {tag}
                      </Pill>
                    ))}
                  </span>
                ) : (
                  <span className="text-tx-3">—</span>
                ),
            },
            {
              key: 'range',
              header: t('list.columns.range', { defaultValue: 'Window' }),
              cell: (v) => <span className="tabular-nums text-tx-1">{formatRange(v.time_range_secs)}</span>,
              width: 90,
            },
            {
              key: 'updated',
              header: t('list.columns.updated', { defaultValue: 'Updated' }),
              cell: (v) => <span className="tabular-nums text-tx-3">{formatMicrosActive(v.updated_at)}</span>,
              width: 170,
            },
            {
              key: 'actions',
              header: t('list.columns.actions', { defaultValue: 'Actions' }),
              cell: (v) => (
                <div className="flex items-center gap-0.5">
                  <IconAction
                    onClick={() => openView(v)}
                    label={t('actions.open', { defaultValue: 'Open' })}
                    icon={<ExternalLink className="h-3 w-3" />}
                  />
                  <IconAction
                    onClick={() => pinMut.mutate(v)}
                    access={restrictActionAccess(
                      editAccess,
                      !pinMut.isPending,
                      t('actions.pending', {
                        defaultValue: 'Another saved-view update is in progress.',
                      }),
                    )}
                    label={
                      v.pinned
                        ? t('actions.unpin', { defaultValue: 'Unpin' })
                        : t('actions.pin', { defaultValue: 'Pin' })
                    }
                    icon={v.pinned ? <PinOff className="h-3 w-3" /> : <Pin className="h-3 w-3" />}
                  />
                  <IconAction
                    access={editAccess}
                    onClick={() => editAccess.allowed && setEditing(v)}
                    label={t('actions.edit', { defaultValue: 'Edit' })}
                    icon={<Pencil className="h-3 w-3" />}
                  />
                  <IconAction
                    access={deleteAccess}
                    onClick={() =>
                      deleteAccess.allowed && setConfirmingDelete(v)
                    }
                    label={t('actions.delete', { defaultValue: 'Delete' })}
                    icon={<Trash2 className="h-3 w-3" />}
                  />
                </div>
              ),
              width: 140,
            },
          ]}
        />
      </ListPage>
      <SavedViewDrawer
        access={editing === 'new' ? createAccess : editAccess}
        open={editing !== null}
        editing={editing === 'new' ? null : editing}
        onClose={() => setEditing(null)}
        onSaved={() => void invalidate()}
      />
      <ConfirmDialog
        open={confirmingDelete !== null}
        onOpenChange={(v) => !v && setConfirmingDelete(null)}
        title={t('confirm.delete_title', { defaultValue: 'Delete saved view?' })}
        description={t('confirm.delete', {
          defaultValue: 'Delete this saved view? This cannot be undone.',
        })}
        confirmLabel={t('actions.delete', { defaultValue: 'Delete' })}
        cancelLabel={t('confirm.cancel', { defaultValue: 'Cancel' })}
        destructive
        busy={deleteMut.isPending}
        disabled={deleteAccess.disabled}
        disabledReason={deleteAccess.reason}
        onConfirm={() => {
          if (deleteAccess.allowed && confirmingDelete) {
            deleteMut.mutate(confirmingDelete.id);
          }
        }}
      />
    </>
  );
}

function IconAction({
  access,
  onClick,
  label,
  icon,
}: {
  access?: ActionAccess;
  onClick: () => void;
  label: string;
  icon: React.ReactNode;
}) {
  return (
    <IconButton
      onClick={onClick}
      disabled={access?.disabled}
      disabledReason={access?.reason}
      aria-label={label}
      title={label}
      className="h-6 w-6"
    >
      {icon}
    </IconButton>
  );
}

/* ─── New / edit drawer ─── */

function SavedViewDrawer({
  access,
  open,
  editing,
  onClose,
  onSaved,
}: {
  access: ActionAccess;
  open: boolean;
  editing: SavedView | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation('saved-views');
  const isEdit = editing !== null;
  const [name, setName] = React.useState('');
  const [language, setLanguage] = React.useState<QueryLanguage>('sql');
  const [statement, setStatement] = React.useState('');
  const [stream, setStream] = React.useState('');
  const [rangeSecs, setRangeSecs] = React.useState('3600');
  const [tags, setTags] = React.useState('');
  const [pinned, setPinned] = React.useState(false);

  React.useEffect(() => {
    setName(editing?.name ?? '');
    setLanguage(editing?.language ?? 'sql');
    setStatement(editing?.statement ?? '');
    setStream(editing?.stream ?? '');
    setRangeSecs(String(editing?.time_range_secs ?? 3600));
    setTags(editing?.tags.join(', ') ?? '');
    setPinned(editing?.pinned ?? false);
  }, [editing]);

  const save = useMutation({
    mutationFn: () => {
      const payload: SavedViewInput = {
        name: name.trim(),
        language,
        statement,
        time_range_secs: Number(rangeSecs) || 3600,
        stream: stream.trim() || null,
        tags: tags
          .split(',')
          .map((s) => s.trim())
          .filter(Boolean),
        pinned,
      };
      return editing ? savedViewsApi.update(editing.id, payload) : savedViewsApi.create(payload);
    },
    onSuccess: () => {
      toast.success(t('toast.saved', { defaultValue: 'Saved view “{{name}}” saved', name: name.trim() }));
      onSaved();
      onClose();
    },
    onError: (err: unknown) => toast.error(toApiError(err).message),
  });

  const initial = {
    name: editing?.name ?? '',
    language: editing?.language ?? 'sql',
    statement: editing?.statement ?? '',
    stream: editing?.stream ?? '',
    rangeSecs: String(editing?.time_range_secs ?? 3600),
    tags: editing?.tags.join(', ') ?? '',
    pinned: editing?.pinned ?? false,
  };
  const dirty =
    name.trim() !== initial.name ||
    language !== initial.language ||
    statement !== initial.statement ||
    stream.trim() !== initial.stream ||
    rangeSecs !== initial.rangeSecs ||
    tags.trim() !== initial.tags ||
    pinned !== initial.pinned;
  const invalid = !name.trim() || !statement.trim();
  const controlsDisabled = access.disabled || save.isPending;
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (access.allowed && dirty && !invalid) save.mutate();
  };

  return (
    <FormDrawer
      open={open}
      onOpenChange={(v) => !v && onClose()}
      title={isEdit ? t('drawer.edit_title', { defaultValue: 'Edit saved view' }) : t('actions.new', { defaultValue: 'New saved view' })}
      subtitle={t('drawer.subtitle', { defaultValue: 'Name a SQL or PromQL query to reuse it later.' })}
      footer={
        <FormSubmitFooter
          busy={save.isPending}
          disabled={access.disabled || !dirty}
          disabledReason={access.reason}
          invalid={invalid}
          onCancel={onClose}
          submitLabel={isEdit ? t('drawer.save', { defaultValue: 'Save changes' }) : t('drawer.create', { defaultValue: 'Create' })}
          formId="saved-view-form"
        />
      }
    >
      <form id="saved-view-form" onSubmit={submit}>
        <FormSection title={t('drawer.sections.identity', { defaultValue: 'Identity' })}>
          <FormField label={t('drawer.fields.name', { defaultValue: 'Name' })} required>
            <FormInput
              value={name}
              disabled={controlsDisabled}
              disabledReason={access.reason}
              onChange={(e) => setName(e.target.value)}
              placeholder="error logs · prod"
              required
            />
          </FormField>
          <FormRow>
            <FormField label={t('drawer.fields.language', { defaultValue: 'Language' })} required>
              <FormSelect
                value={language}
                disabled={controlsDisabled}
                disabledReason={access.reason}
                onChange={(v) => setLanguage(v as QueryLanguage)}
                options={[
                  { value: 'sql', label: 'SQL' },
                  { value: 'promql', label: 'PromQL' },
                ]}
              />
            </FormField>
            <FormField label={t('drawer.fields.range', { defaultValue: 'Default window' })}>
              <FormSelect
                value={rangeSecs}
                disabled={controlsDisabled}
                disabledReason={access.reason}
                onChange={setRangeSecs}
                options={TIME_RANGES}
              />
            </FormField>
          </FormRow>
        </FormSection>

        <FormSection title={t('drawer.sections.query', { defaultValue: 'Query' })}>
          <FormField label={t('drawer.fields.statement', { defaultValue: 'Statement' })} required>
            <CodeEditor
              value={statement}
              onChange={setStatement}
              readOnly={controlsDisabled}
              language={language}
              label={language.toUpperCase()}
              ariaLabel={t('drawer.fields.statement', { defaultValue: 'Statement' })}
              minHeight={160}
              maxHeight={320}
              compact
              resizable
              showHeader={false}
            />
          </FormField>
          <FormField label={t('drawer.fields.stream', { defaultValue: 'Stream (optional)' })} hint={t('drawer.hints.stream', { defaultValue: 'Limits partition pruning when the view is opened.' })}>
            <FormInput
              value={stream}
              disabled={controlsDisabled}
              disabledReason={access.reason}
              onChange={(e) => setStream(e.target.value)}
              placeholder="app_logs"
            />
          </FormField>
        </FormSection>

        <FormSection title={t('drawer.sections.organize', { defaultValue: 'Organize' })}>
          <FormField label={t('drawer.fields.tags', { defaultValue: 'Tags' })} hint={t('drawer.hints.tags', { defaultValue: 'Comma-separated.' })}>
            <FormInput
              value={tags}
              disabled={controlsDisabled}
              disabledReason={access.reason}
              onChange={(e) => setTags(e.target.value)}
              placeholder="triage, errors"
            />
          </FormField>
          <FormChecklist
            options={[{ value: 'pinned', label: t('drawer.fields.pinned', { defaultValue: 'Pin to top' }) }]}
            selected={pinned ? ['pinned'] : []}
            disabled={controlsDisabled}
            disabledReason={access.reason}
            onChange={(next) => setPinned(next.includes('pinned'))}
          />
        </FormSection>
      </form>
    </FormDrawer>
  );
}
