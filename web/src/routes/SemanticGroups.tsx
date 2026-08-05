import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Pencil, Plus, Trash2, Upload, X } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, DataTable } from '@/admin';
import * as api from '@/api/semanticGroups';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { type ProductStateProps } from '@/product/states';
import { ListPage } from '@/product/templates';
import { ChromeButton, IconButton, Pill } from '@/shell/chrome';
import { CodeEditor } from '@/shell/codeEditor';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormSection,
  FormSelect,
  FormSubmitFooter,
} from '@/shell/FormDrawer';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';

import { AlertsSubNav } from './alerts/Layout';

const OP_OPTIONS: Array<{ value: api.MatchOp; label: string }> = [
  { value: 'eq', label: '= (eq)' },
  { value: 'neq', label: '!= (neq)' },
  { value: 're', label: '=~ (regex)' },
  { value: 'nre', label: '!~ (not regex)' },
];

const OP_SYMBOL: Record<api.MatchOp, string> = { eq: '=', neq: '!=', re: '=~', nre: '!~' };

function matchersSummary(matchers: api.LabelMatcher[]): string {
  if (matchers.length === 0) return '*';
  return matchers.map((m) => `${m.label}${OP_SYMBOL[m.op]}${m.value}`).join(', ');
}

export function SemanticGroups() {
  const { t } = useTranslation('semantic-groups');
  const qc = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'alerts.manage' });
  const [editing, setEditing] = React.useState<api.SemanticGroup | 'new' | null>(null);
  const [importing, setImporting] = React.useState(false);
  const [confirmingDelete, setConfirmingDelete] = React.useState<api.SemanticGroup | null>(null);

  const groupsQuery = useQuery({
    queryKey: ['semantic-groups'],
    queryFn: () => api.list(),
  });
  const invalidate = () => qc.invalidateQueries({ queryKey: ['semantic-groups'] });

  const deleteMut = useMutation({
    mutationFn: (id: string) => api.remove(id),
    onSuccess: () => {
      toast.success(t('toast.deleted'));
      setConfirmingDelete(null);
      void invalidate();
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  const rows = groupsQuery.data ?? [];
  const tableState = queryStateFor({
    isLoading: groupsQuery.isLoading,
    isError: groupsQuery.isError,
    data: rows,
  });
  const listState: ProductStateProps | null =
    tableState === 'loading'
      ? { variant: 'loading' }
      : tableState === 'error'
        ? { variant: 'error', error: groupsQuery.error }
        : tableState === 'empty'
          ? {
              variant: 'empty',
              title: t('states.empty_title'),
              description: t('states.empty_description'),
              action: (
                <ChromeButton
                  variant="primary"
                  disabled={manageAccess.disabled}
                  disabledReason={manageAccess.reason}
                  onClick={() => setEditing('new')}
                >
                  <Plus className="h-3 w-3" /> {t('actions.new')}
                </ChromeButton>
              ),
            }
          : null;

  return (
    <>
      <ListPage
        title={t('title')}
        subtitle={t('subtitle') as string}
        subnav={<AlertsSubNav />}
        toolbar={
          <>
            <ChromeButton
              disabled={manageAccess.disabled}
              disabledReason={manageAccess.reason}
              onClick={() => setImporting(true)}
            >
              <Upload className="h-3 w-3" /> {t('actions.import')}
            </ChromeButton>
            <ChromeButton
              variant="primary"
              disabled={manageAccess.disabled}
              disabledReason={manageAccess.reason}
              onClick={() => setEditing('new')}
            >
              <Plus className="h-3 w-3" /> {t('actions.new')}
            </ChromeButton>
          </>
        }
        state={listState}
      >
        <DataTable
          rows={rows}
          rowKey={(g) => g.id}
          columns={[
            {
              key: 'name',
              header: t('list.columns.name'),
              cell: (g) => <span className="text-tx-0">{g.name}</span>,
            },
            {
              key: 'matchers',
              header: t('list.columns.matchers'),
              cell: (g) => <span className="text-tx-1">{matchersSummary(g.matchers)}</span>,
            },
            {
              key: 'group_by',
              header: t('list.columns.group_by'),
              cell: (g) => (
                <span className="text-blue-soft">
                  {g.group_by.length > 0 ? g.group_by.join(', ') : '—'}
                </span>
              ),
            },
            {
              key: 'enabled',
              header: t('list.columns.enabled'),
              cell: (g) => (
                <Pill tone={g.enabled ? 'green' : 'dim'}>
                  {g.enabled ? t('enabled.yes') : t('enabled.no')}
                </Pill>
              ),
              width: 110,
            },
            {
              key: 'actions',
              header: t('list.columns.actions'),
              cell: (g) => (
                <div className="flex items-center gap-1">
                  <IconButton
                    disabled={manageAccess.disabled}
                    disabledReason={manageAccess.reason}
                    onClick={() => setEditing(g)}
                    aria-label={t('actions.edit_named', { name: g.name })}
                  >
                    <Pencil className="h-3 w-3" />
                  </IconButton>
                  <IconButton
                    disabled={manageAccess.disabled}
                    disabledReason={manageAccess.reason}
                    onClick={() => setConfirmingDelete(g)}
                    aria-label={t('actions.delete_named', { name: g.name })}
                  >
                    <Trash2 className="h-3 w-3" />
                  </IconButton>
                </div>
              ),
              width: 96,
            },
          ]}
        />
      </ListPage>

      <GroupDrawer
        open={editing !== null}
        editing={editing === 'new' ? null : editing}
        onClose={() => setEditing(null)}
        onSaved={() => void invalidate()}
      />
      <ImportDrawer
        open={importing}
        onClose={() => setImporting(false)}
        onImported={() => void invalidate()}
      />
      <ConfirmDialog
        open={confirmingDelete !== null}
        onOpenChange={(v) => !v && setConfirmingDelete(null)}
        title={t('confirm.delete_title')}
        description={t('confirm.delete')}
        confirmLabel={t('actions.delete')}
        cancelLabel={t('confirm.cancel')}
        destructive
        busy={deleteMut.isPending}
        disabled={manageAccess.disabled}
        disabledReason={manageAccess.reason}
        onConfirm={() => {
          if (confirmingDelete && manageAccess.allowed) {
            deleteMut.mutate(confirmingDelete.id);
          }
        }}
      />
    </>
  );
}

/* ─── create / edit drawer ─── */

interface MatcherRow {
  label: string;
  op: api.MatchOp;
  value: string;
}

function GroupDrawer({
  open,
  editing,
  onClose,
  onSaved,
}: {
  open: boolean;
  editing: api.SemanticGroup | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation('semantic-groups');
  const manageAccess = useActionAccess({ permission: 'alerts.manage' });
  const isEdit = editing !== null;
  const [name, setName] = React.useState('');
  const [enabled, setEnabled] = React.useState(true);
  const [matchers, setMatchers] = React.useState<MatcherRow[]>([]);
  const [groupBy, setGroupBy] = React.useState('');

  React.useEffect(() => {
    setName(editing?.name ?? '');
    setEnabled(editing?.enabled ?? true);
    setMatchers(editing?.matchers.map((m) => ({ ...m })) ?? []);
    setGroupBy(editing?.group_by.join(', ') ?? '');
  }, [editing, open]);

  const save = useMutation({
    mutationFn: () => {
      const payload: api.SemanticGroupInput = {
        name: name.trim(),
        enabled,
        matchers: matchers
          .map((m) => ({ label: m.label.trim(), op: m.op, value: m.value }))
          .filter((m) => m.label !== ''),
        group_by: groupBy
          .split(',')
          .map((s) => s.trim())
          .filter(Boolean),
      };
      return editing ? api.update(editing.id, payload) : api.create(payload);
    },
    onSuccess: () => {
      toast.success(t('toast.saved', { name }));
      onSaved();
      onClose();
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!manageAccess.allowed) return;
    if (!name.trim()) {
      toast.error(t('errors.name_required'));
      return;
    }
    save.mutate();
  };

  const setMatcher = (i: number, patch: Partial<MatcherRow>) =>
    setMatchers((rows) => rows.map((m, idx) => (idx === i ? { ...m, ...patch } : m)));

  return (
    <FormDrawer
      open={open}
      onOpenChange={(v) => !v && onClose()}
      title={isEdit ? t('drawer.edit_title', { name: editing.name }) : t('actions.new')}
      subtitle={t('drawer.subtitle')}
      footer={
        <FormSubmitFooter
          busy={save.isPending}
          disabled={manageAccess.disabled}
          disabledReason={manageAccess.reason}
          invalid={!name.trim()}
          onCancel={onClose}
          submitLabel={isEdit ? t('drawer.save') : t('drawer.create')}
          formId="semantic-group-form"
        />
      }
    >
      <form id="semantic-group-form" onSubmit={submit}>
        <fieldset
          disabled={manageAccess.disabled}
          aria-disabled={manageAccess.disabled || undefined}
          className="contents"
        >
        <FormSection title={t('drawer.sections.identity')}>
          <FormField label={t('drawer.fields.name')} required>
            <FormInput
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t('drawer.placeholders.name')}
              required
            />
          </FormField>
          <FormField label={t('drawer.fields.enabled')} hint={t('drawer.hints.enabled')}>
            <label className="flex items-center gap-2 font-sans text-xs text-tx-1">
              <input
                type="checkbox"
                checked={enabled}
                onChange={(e) => setEnabled(e.target.checked)}
                className="h-3.5 w-3.5 accent-indigo"
              />
              {enabled ? t('enabled.yes') : t('enabled.no')}
            </label>
          </FormField>
        </FormSection>

        <FormSection
          title={t('drawer.sections.matchers')}
          description={t('drawer.matchers_description')}
        >
          <div className="flex flex-col gap-2">
            {matchers.map((m, i) => (
              <div key={i} className="flex items-center gap-2">
                <FormInput
                  value={m.label}
                  onChange={(e) => setMatcher(i, { label: e.target.value })}
                  placeholder={t('drawer.placeholders.matcher_label')}
                  className="flex-1"
                  aria-label={t('drawer.fields.matcher_label')}
                />
                <div className="w-32 shrink-0">
                  <FormSelect
                    value={m.op}
                    onChange={(v) => setMatcher(i, { op: v as api.MatchOp })}
                    options={OP_OPTIONS}
                  />
                </div>
                <FormInput
                  value={m.value}
                  onChange={(e) => setMatcher(i, { value: e.target.value })}
                  placeholder={t('drawer.placeholders.matcher_value')}
                  className="flex-1"
                  aria-label={t('drawer.fields.matcher_value')}
                />
                <button
                  type="button"
                  onClick={() => setMatchers((rows) => rows.filter((_, idx) => idx !== i))}
                  className="flex h-7 w-7 shrink-0 items-center justify-center rounded text-tx-3 hover:bg-bg-3 hover:text-red-soft"
                  aria-label={t('drawer.remove_matcher')}
                >
                  <X className="h-3.5 w-3.5" />
                </button>
              </div>
            ))}
            <div>
              <ChromeButton
                type="button"
                onClick={() => setMatchers((rows) => [...rows, { label: '', op: 'eq', value: '' }])}
              >
                <Plus className="h-3 w-3" /> {t('drawer.add_matcher')}
              </ChromeButton>
            </div>
          </div>
        </FormSection>

        <FormSection title={t('drawer.sections.group_by')} description={t('drawer.group_by_description')}>
          <FormField label={t('drawer.fields.group_by')} hint={t('drawer.hints.group_by')}>
            <FormInput
              value={groupBy}
              onChange={(e) => setGroupBy(e.target.value)}
              placeholder={t('drawer.placeholders.group_by')}
            />
          </FormField>
        </FormSection>
        </fieldset>
      </form>
    </FormDrawer>
  );
}

/* ─── import drawer ─── */

function ImportDrawer({
  open,
  onClose,
  onImported,
}: {
  open: boolean;
  onClose: () => void;
  onImported: () => void;
}) {
  const { t } = useTranslation('semantic-groups');
  const manageAccess = useActionAccess({ permission: 'alerts.manage' });
  const [text, setText] = React.useState('');
  const [parseError, setParseError] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (open) {
      setText('');
      setParseError(null);
    }
  }, [open]);

  const importMut = useMutation({
    mutationFn: (groups: api.SemanticGroupInput[]) => api.importGroups(groups),
    onSuccess: (r) => {
      toast.success(t('import.toast_imported', { count: r.imported }));
      onImported();
      onClose();
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!manageAccess.allowed) return;
    let parsed: unknown;
    try {
      parsed = JSON.parse(text);
    } catch (err) {
      setParseError(t('import.invalid_json', { message: (err as Error).message }));
      return;
    }
    // Accept either a bare array or { groups: [...] }.
    const groups = Array.isArray(parsed)
      ? parsed
      : (parsed as { groups?: unknown }).groups;
    if (!Array.isArray(groups) || groups.length === 0) {
      setParseError(t('import.expects_array'));
      return;
    }
    setParseError(null);
    importMut.mutate(groups as api.SemanticGroupInput[]);
  };

  return (
    <FormDrawer
      open={open}
      onOpenChange={(v) => !v && onClose()}
      title={t('import.title')}
      subtitle={t('import.subtitle')}
      footer={
        <FormSubmitFooter
          busy={importMut.isPending}
          disabled={manageAccess.disabled}
          disabledReason={manageAccess.reason}
          invalid={!text.trim()}
          onCancel={onClose}
          submitLabel={t('import.submit')}
          formId="semantic-group-import-form"
        />
      }
    >
      <form id="semantic-group-import-form" onSubmit={submit} className="flex flex-col gap-2">
        <CodeEditor
          value={text}
          onChange={setText}
          language="json"
          label={t('import.editor_label')}
          ariaLabel={t('import.editor_label')}
          placeholder={t('import.placeholder')}
          minHeight={280}
          maxHeight={520}
          readOnly={manageAccess.disabled}
        />
        {parseError && <div className="font-sans text-xs text-red-soft">{parseError}</div>}
      </form>
    </FormDrawer>
  );
}
