import { useMutation, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as mutesApi from '@/api/mutes';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
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
import { cn } from '@/shell/lib/cn';
import { MatchersEditor } from '@/shell/MatchersEditor';
import { TimezoneField } from '@/shell/TimezoneField';
import { Checkbox } from '@/shell/ui/checkbox';
import { toast } from '@/shell/ui/sonner';
import type { LabelMatcher, MuteRule, MuteWindow } from '@/types/alerting';

import { localInputToMicros, microsToLocalInput } from '../schedule';
import {
  DEFAULT_SILENCE_TIMEZONE,
  clampHour,
  defaultSilenceWindow,
  weekdayLabel,
} from './window';

/** The form of one silence policy: what it catches, when, and why. */
export function SilenceDrawer({
  open,
  editing,
  onClose,
}: {
  open: boolean;
  editing: MuteRule | null;
  onClose: () => void;
}) {
  const { t, i18n } = useTranslation('alerts');
  const queryClient = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'alerts.silence' });
  const [name, setName] = React.useState('');
  const [enabled, setEnabled] = React.useState(true);
  const [matchers, setMatchers] = React.useState<LabelMatcher[]>([]);
  const [window, setWindow] = React.useState<MuteWindow>(() => defaultSilenceWindow());
  const [comment, setComment] = React.useState('');

  React.useEffect(() => {
    if (!open) return;

    setName(editing?.name ?? '');
    setEnabled(editing?.enabled ?? true);
    setMatchers(editing?.matchers?.map((matcher) => ({ ...matcher })) ?? []);
    setWindow(editing?.window ? { ...editing.window } : defaultSilenceWindow());
    setComment(editing?.comment ?? '');
  }, [open, editing]);

  const namedMatchers = matchers.filter((matcher) => matcher.label.trim() !== '');
  const save = useMutation({
    mutationFn: () => {
      const payload: mutesApi.MuteRuleInput = {
        name: name.trim(),
        enabled,
        matchers: namedMatchers,
        window,
        comment: comment.trim(),
      };

      return editing ? mutesApi.update(editing.id, payload) : mutesApi.create(payload);
    },
    onSuccess: () => {
      toast.success(editing ? t('silences.toast_updated') : t('silences.toast_created'));
      void queryClient.invalidateQueries({ queryKey: ['alert-mutes'] });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const submit = (event: React.FormEvent) => {
    event.preventDefault();

    if (!manageAccess.allowed) return;
    if (!name.trim()) {
      toast.error(t('silences.errors.name_required'));
      return;
    }
    if (namedMatchers.length === 0) {
      toast.error(t('silences.errors.matcher_required'));
      return;
    }
    if (window.type === 'fixed' && window.end <= window.start) {
      toast.error(t('silences.errors.window_range'));
      return;
    }

    save.mutate();
  };

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={editing ? t('silences.edit_title') : t('silences.drawer_title')}
      subtitle={t('silences.drawer_subtitle')}
      width={760}
      footer={
        <FormSubmitFooter
          busy={save.isPending}
          disabled={manageAccess.disabled}
          disabledReason={manageAccess.reason}
          invalid={!name.trim() || namedMatchers.length === 0}
          onCancel={onClose}
          submitLabel={editing ? t('silences.save') : t('silences.create')}
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
          <FormSection title={t('silences.sections.identity')}>
            <FormField label={t('silences.fields.name')} required>
              <FormInput
                value={name}
                onChange={(event) => setName(event.target.value)}
                placeholder="maintenance_db"
                required
              />
            </FormField>
            <label className="flex min-h-9 items-center gap-2 rounded-md bg-[var(--control-surface)] px-2.5 py-2 font-sans text-sm text-tx-0">
              <Checkbox checked={enabled} onCheckedChange={(next) => setEnabled(next === true)} />
              <span>{t('silences.fields.enabled')}</span>
            </label>
          </FormSection>

          <FormSection title={t('silences.sections.matchers')}>
            <MatchersEditor matchers={matchers} onChange={setMatchers} />
          </FormSection>

          <FormSection title={t('silences.sections.window')}>
            <FormField label={t('silences.fields.window_type')}>
              <FormSelect
                value={window.type}
                onChange={(value) =>
                  setWindow(
                    value === 'recurring'
                      ? {
                          type: 'recurring',
                          timezone: DEFAULT_SILENCE_TIMEZONE,
                          weekday_mask: 127,
                          hour_start: 0,
                          hour_end: 24,
                        }
                      : defaultSilenceWindow(),
                  )
                }
                options={[
                  { value: 'fixed', label: t('silences.window_kinds.fixed') },
                  { value: 'recurring', label: t('silences.window_kinds.recurring') },
                ]}
              />
            </FormField>
            {window.type === 'fixed' ? (
              <div className="grid grid-cols-2 gap-2">
                <FormField label={t('silences.fields.start')} required>
                  <DateTimePicker
                    value={microsToLocalInput(window.start)}
                    onChange={(value) => setWindow({ ...window, start: localInputToMicros(value) })}
                    required
                  />
                </FormField>
                <FormField label={t('silences.fields.end')} required>
                  <DateTimePicker
                    value={microsToLocalInput(window.end)}
                    onChange={(value) => setWindow({ ...window, end: localInputToMicros(value) })}
                    required
                  />
                </FormField>
              </div>
            ) : (
              <div className="flex flex-col gap-2">
                <FormField label={t('silences.fields.timezone')}>
                  <TimezoneField
                    label={t('silences.fields.timezone')}
                    value={window.timezone}
                    onChange={(timezone) => setWindow({ ...window, timezone })}
                  />
                </FormField>
                <div role="group" aria-label={t('silences.fields.weekdays')} className="flex flex-wrap gap-1">
                  {[0, 1, 2, 3, 4, 5, 6].map((day) => {
                    const on = (window.weekday_mask & (1 << day)) !== 0;

                    return (
                      <button
                        key={day}
                        type="button"
                        aria-pressed={on}
                        onClick={() => setWindow({ ...window, weekday_mask: window.weekday_mask ^ (1 << day) })}
                        className={cn(
                          'min-w-[44px] rounded-md px-2.5 py-1.5 font-sans text-xs outline-none transition-colors',
                          on
                            ? 'bg-indigo-dim font-strong text-indigo-soft'
                            : 'bg-[var(--control-surface)] text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3',
                        )}
                      >
                        {weekdayLabel(day, i18n.language)}
                      </button>
                    );
                  })}
                </div>
                <div className="grid grid-cols-2 gap-2">
                  <FormField label={t('silences.fields.hour_start')}>
                    <FormInput
                      type="number"
                      min={0}
                      max={24}
                      value={String(window.hour_start)}
                      onChange={(event) => setWindow({ ...window, hour_start: clampHour(event.target.value) })}
                    />
                  </FormField>
                  <FormField label={t('silences.fields.hour_end')}>
                    <FormInput
                      type="number"
                      min={0}
                      max={24}
                      value={String(window.hour_end)}
                      onChange={(event) => setWindow({ ...window, hour_end: clampHour(event.target.value) })}
                    />
                  </FormField>
                </div>
              </div>
            )}
          </FormSection>

          <FormSection title={t('silences.sections.comment')} className="mb-0">
            <FormField label={t('silences.fields.comment')}>
              <FormTextarea
                value={comment}
                onChange={(event) => setComment(event.target.value)}
                rows={2}
                placeholder={t('silences.comment_placeholder')}
              />
            </FormField>
          </FormSection>
        </fieldset>
      </form>
    </FormDrawer>
  );
}
