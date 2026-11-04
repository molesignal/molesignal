import { useMutation, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as mutesApi from '@/api/mutes';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormSection,
  FormSubmitFooter,
  FormTextarea,
} from '@/shell/FormDrawer';
import { MatchersEditor } from '@/shell/MatchersEditor';
import { Checkbox } from '@/shell/ui/checkbox';
import { toast } from '@/shell/ui/sonner';
import type { LabelMatcher, MuteRule, MuteWindow } from '@/types/alerting';

import { defaultSilenceWindow } from './window';
import { SilenceWindowFields, windowProblem } from './WindowFields';

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
  const { t } = useTranslation('alerts');
  const queryClient = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'alerts.mutes.manage' });
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
    const problem = windowProblem(window);
    if (problem) {
      toast.error(t(`silences.errors.${problem}`));
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
            <SilenceWindowFields window={window} onChange={setWindow} />
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
