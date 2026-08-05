import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, DataTable } from '@/admin';
import * as mutesApi from '@/api/mutes';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ProductState, productStateFor } from '@/product/states';
import { ChromeButton, Pill } from '@/shell/chrome';
import { DateTimePicker } from '@/shell/DateTimePicker';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormSection,
  FormSelect,
  FormSubmitFooter,
  FormTextarea,
} from '@/shell/FormDrawer';
import { MatchersEditor } from '@/shell/MatchersEditor';
import { PageBody, PageHeader } from '@/shell/PageHeader';
import { queryStateFor } from '@/shell/query/State';
import { Checkbox } from '@/shell/ui/checkbox';
import { toast } from '@/shell/ui/sonner';
import type { LabelMatcher, MuteRule, MuteWindow } from '@/types/alerting';

import { AlertsSubNav } from './Layout';
import { localInputToMicros, microsToLocalInput } from './schedule';

const DEFAULT_TZ = 'UTC';

function weekdayLabel(i: number): string {
  return new Intl.DateTimeFormat(undefined, { weekday: 'short' }).format(new Date(Date.UTC(2023, 0, 1 + i)));
}

function defaultWindow(): MuteWindow {
  const now = Date.now();
  return { type: 'fixed', start: now * 1000, end: (now + 60 * 60 * 1000) * 1000 };
}

function windowSummary(w: MuteWindow): string {
  if (w.type === 'fixed') {
    return `${new Date(w.start / 1000).toLocaleString()} → ${new Date(w.end / 1000).toLocaleString()}`;
  }
  const days = [0, 1, 2, 3, 4, 5, 6]
    .filter((d) => (w.weekday_mask & (1 << d)) !== 0)
    .map(weekdayLabel);
  return `${days.join(', ') || '—'} · ${w.hour_start}:00–${w.hour_end}:00 ${w.timezone}`;
}

export function AlertsSilences() {
  const { t } = useTranslation('alerts');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'alerts.silence' });
  const [creating, setCreating] = React.useState(false);
  const [editing, setEditing] = React.useState<MuteRule | null>(null);
  const [removing, setRemoving] = React.useState<MuteRule | null>(null);

  const q = useQuery({ queryKey: ['alert-mutes'], queryFn: () => mutesApi.list() });
  const rows = q.data ?? [];
  const state = queryStateFor({ isLoading: q.isLoading, isError: q.isError, data: rows });
  const pageState = productStateFor(state, {
    error: q.error,
    emptyTitle: t('silences.empty_title', { defaultValue: 'No silences' }),
    emptyDescription: t('silences.empty_description', {
      defaultValue: 'Suppress notifications for matching incidents during maintenance or known noise.',
    }),
  });

  const remove = useMutation({
    mutationFn: (id: string) => mutesApi.remove(id),
    onSuccess: () => {
      toast.success(tc('status.deleted'));
      void qc.invalidateQueries({ queryKey: ['alert-mutes'] });
      setRemoving(null);
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  return (
    <>
      <PageHeader
        title={t('silences.title', { defaultValue: 'Silences' })}
        subtitle={t('silences.subtitle', { defaultValue: 'Mute matching incidents for a window' })}
        toolbar={
          <ChromeButton
            variant="primary"
            disabled={manageAccess.disabled}
            disabledReason={manageAccess.reason}
            onClick={() => setCreating(true)}
          >
            {t('silences.new_silence', { defaultValue: 'New silence' })}
          </ChromeButton>
        }
      />
      <AlertsSubNav />
      <PageBody>
        {pageState ? (
          <ProductState {...pageState} />
        ) : (
          <DataTable
            rows={rows}
            rowKey={(r) => r.id}
            onRowClick={(r) => setEditing(r)}
            isRowClickDisabled={() => manageAccess.disabled}
            rowClickDisabledReason={() => manageAccess.reason}
            columns={[
              { key: 'name', header: t('silences.columns.name', { defaultValue: 'Name' }), cell: (r) => r.name },
              {
                key: 'enabled',
                header: t('silences.columns.status', { defaultValue: 'Status' }),
                cell: (r) =>
                  r.enabled ? (
                    <Pill tone="green">{tc('status.on')}</Pill>
                  ) : (
                    <Pill tone="dim">{tc('status.off')}</Pill>
                  ),
                width: 90,
              },
              {
                key: 'window',
                header: t('silences.columns.window', { defaultValue: 'Window' }),
                cell: (r) => <span className="text-xs text-tx-2">{windowSummary(r.window)}</span>,
              },
              {
                key: 'actions',
                header: '',
                width: 80,
                cell: (r) => (
                  <ChromeButton
                    size="sm"
                    disabled={manageAccess.disabled}
                    disabledReason={manageAccess.reason}
                    onClick={(e) => {
                      e.stopPropagation();
                      setRemoving(r);
                    }}
                  >
                    {tc('actions.delete')}
                  </ChromeButton>
                ),
              },
            ]}
          />
        )}
      </PageBody>
      <SilenceDrawer
        open={creating || editing !== null}
        editing={editing}
        onClose={() => {
          setCreating(false);
          setEditing(null);
        }}
      />
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(v) => !v && setRemoving(null)}
        destructive
        title={t('silences.delete_title', { defaultValue: 'Delete silence' })}
        description={removing?.name ?? ''}
        confirmLabel={tc('actions.delete')}
        busy={remove.isPending}
        disabled={manageAccess.disabled}
        disabledReason={manageAccess.reason}
        onConfirm={() => {
          if (removing && manageAccess.allowed) remove.mutate(removing.id);
        }}
      />
    </>
  );
}

function SilenceDrawer({
  open,
  editing,
  onClose,
}: {
  open: boolean;
  editing: MuteRule | null;
  onClose: () => void;
}) {
  const { t } = useTranslation('alerts');
  const qc = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'alerts.silence' });
  const [name, setName] = React.useState('');
  const [enabled, setEnabled] = React.useState(true);
  const [matchers, setMatchers] = React.useState<LabelMatcher[]>([]);
  const [window, setWindow] = React.useState<MuteWindow>(() => defaultWindow());
  const [comment, setComment] = React.useState('');

  React.useEffect(() => {
    if (!open) return;
    setName(editing?.name ?? '');
    setEnabled(editing?.enabled ?? true);
    setMatchers(editing?.matchers?.map((m) => ({ ...m })) ?? []);
    setWindow(editing?.window ? { ...editing.window } : defaultWindow());
    setComment(editing?.comment ?? '');
  }, [open, editing]);

  const save = useMutation({
    mutationFn: () => {
      const payload: mutesApi.MuteRuleInput = {
        name: name.trim(),
        enabled,
        matchers: matchers.filter((m) => m.label.trim() !== ''),
        window,
        comment: comment.trim(),
      };
      return editing ? mutesApi.update(editing.id, payload) : mutesApi.create(payload);
    },
    onSuccess: () => {
      toast.success(editing ? t('silences.toast_updated', { defaultValue: 'Silence updated' }) : t('silences.toast_created', { defaultValue: 'Silence created' }));
      void qc.invalidateQueries({ queryKey: ['alert-mutes'] });
      onClose();
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!manageAccess.allowed) return;
    if (!name.trim()) {
      toast.error(t('silences.errors.name_required', { defaultValue: 'Name is required.' }));
      return;
    }
    if (matchers.filter((m) => m.label.trim() !== '').length === 0) {
      toast.error(t('silences.errors.matcher_required', { defaultValue: 'Add at least one matcher.' }));
      return;
    }
    if (window.type === 'fixed' && window.end <= window.start) {
      toast.error(t('silences.errors.window_range', { defaultValue: 'End must be after start.' }));
      return;
    }
    save.mutate();
  };

  return (
    <FormDrawer
      open={open}
      onOpenChange={(v) => !v && onClose()}
      title={editing ? t('silences.edit_title', { defaultValue: 'Edit silence' }) : t('silences.drawer_title', { defaultValue: 'New silence' })}
      subtitle={t('silences.drawer_subtitle', { defaultValue: 'Incidents are still recorded; only delivery is paused.' }) as string}
      width={760}
      footer={
        <FormSubmitFooter
          busy={save.isPending}
          disabled={manageAccess.disabled}
          disabledReason={manageAccess.reason}
          invalid={
            !name.trim()
            || matchers.every((matcher) => !matcher.label.trim())
          }
          onCancel={onClose}
          submitLabel={editing ? t('silences.save', { defaultValue: 'Save silence' }) : t('silences.create', { defaultValue: 'Create silence' })}
          formId="silence-form"
        />
      }
    >
      <form id="silence-form" onSubmit={submit}>
        <fieldset
          disabled={manageAccess.disabled}
          aria-disabled={manageAccess.disabled || undefined}
          className="contents"
        >
        <FormSection title={t('silences.sections.identity', { defaultValue: 'Identity' })}>
          <FormField label={t('silences.fields.name', { defaultValue: 'Name' })} required>
            <FormInput value={name} onChange={(e) => setName(e.target.value)} placeholder="maintenance_db" required />
          </FormField>
          <label className="flex min-h-9 items-center gap-2 rounded-md border border-bd-0 bg-bg-2 px-2.5 py-2 font-sans text-xs text-tx-0">
            <Checkbox checked={enabled} onCheckedChange={(next) => setEnabled(next === true)} />
            <span>{t('silences.fields.enabled', { defaultValue: 'Enabled' })}</span>
          </label>
        </FormSection>

        <FormSection title={t('silences.sections.matchers', { defaultValue: 'Match' })}>
          <MatchersEditor matchers={matchers} onChange={setMatchers} />
        </FormSection>

        <FormSection title={t('silences.sections.window', { defaultValue: 'Window' })}>
          <FormField label={t('silences.fields.window_type', { defaultValue: 'Window type' })}>
            <FormSelect
              value={window.type}
              onChange={(v) =>
                setWindow(
                  v === 'recurring'
                    ? { type: 'recurring', timezone: DEFAULT_TZ, weekday_mask: 127, hour_start: 0, hour_end: 24 }
                    : defaultWindow(),
                )
              }
              options={[
                { value: 'fixed', label: t('silences.window_kinds.fixed', { defaultValue: 'Fixed range' }) },
                { value: 'recurring', label: t('silences.window_kinds.recurring', { defaultValue: 'Recurring' }) },
              ]}
            />
          </FormField>
          {window.type === 'fixed' ? (
            <div className="grid grid-cols-2 gap-2">
              <FormField label={t('silences.fields.start', { defaultValue: 'Start' })} required>
                <DateTimePicker
                  value={microsToLocalInput(window.start)}
                  onChange={(value) =>
                    setWindow({
                      ...window,
                      start: localInputToMicros(value),
                    })
                  }
                  required
                />
              </FormField>
              <FormField label={t('silences.fields.end', { defaultValue: 'End' })} required>
                <DateTimePicker
                  value={microsToLocalInput(window.end)}
                  onChange={(value) =>
                    setWindow({
                      ...window,
                      end: localInputToMicros(value),
                    })
                  }
                  required
                />
              </FormField>
            </div>
          ) : (
            <div className="flex flex-col gap-2">
              <FormField label={t('silences.fields.timezone', { defaultValue: 'Timezone (IANA)' })}>
                <FormInput
                  value={window.timezone}
                  onChange={(e) => setWindow({ ...window, timezone: e.target.value })}
                  className="font-mono"
                  placeholder="UTC"
                />
              </FormField>
              <div className="flex flex-wrap gap-1">
                {[0, 1, 2, 3, 4, 5, 6].map((d) => {
                  const on = (window.weekday_mask & (1 << d)) !== 0;
                  return (
                    <button
                      key={d}
                      type="button"
                      onClick={() => setWindow({ ...window, weekday_mask: window.weekday_mask ^ (1 << d) })}
                      className={
                        on
                          ? 'rounded-md border border-indigo/45 bg-indigo/15 px-2 py-1 font-sans text-xs text-indigo-soft'
                          : 'rounded-md border border-bd-0 bg-bg-1 px-2 py-1 font-sans text-xs text-tx-3 hover:text-tx-1'
                      }
                    >
                      {weekdayLabel(d)}
                    </button>
                  );
                })}
              </div>
              <div className="grid grid-cols-2 gap-2">
                <FormField label={t('silences.fields.hour_start', { defaultValue: 'Hour start (0–24)' })}>
                  <FormInput
                    type="number"
                    min={0}
                    max={24}
                    value={String(window.hour_start)}
                    onChange={(e) => setWindow({ ...window, hour_start: clampHour(e.target.value) })}
                  />
                </FormField>
                <FormField label={t('silences.fields.hour_end', { defaultValue: 'Hour end (0–24)' })}>
                  <FormInput
                    type="number"
                    min={0}
                    max={24}
                    value={String(window.hour_end)}
                    onChange={(e) => setWindow({ ...window, hour_end: clampHour(e.target.value) })}
                  />
                </FormField>
              </div>
            </div>
          )}
        </FormSection>

        <FormSection title={t('silences.sections.comment', { defaultValue: 'Comment' })} className="mb-0">
          <FormField label={t('silences.fields.comment', { defaultValue: 'Reason' })}>
            <FormTextarea
              value={comment}
              onChange={(e) => setComment(e.target.value)}
              rows={2}
              placeholder={t('silences.comment_placeholder', { defaultValue: 'Why is this muted?' })}
            />
          </FormField>
        </FormSection>
        </fieldset>
      </form>
    </FormDrawer>
  );
}

function clampHour(value: string): number {
  const n = Math.floor(Number(value));
  if (!Number.isFinite(n)) return 0;
  return Math.min(24, Math.max(0, n));
}
