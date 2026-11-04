import { useMutation, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as inhibitionsApi from '@/api/inhibitions';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
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
import { Checkbox } from '@/shell/ui/checkbox';
import { toast } from '@/shell/ui/sonner';
import type { InhibitRule, Severity } from '@/types/alerting';

import { cleanInput, equalFromText, newRuleInput, ruleInput, ruleProblem } from './model';

export const INHIBIT_RULES_KEY = ['alerts', 'inhibit-rules'] as const;

const SEVERITIES: readonly Severity[] = ['critical', 'error', 'warning', 'info'];

const CHECK_ROW = 'flex min-h-9 items-center gap-2 rounded-md bg-[var(--control-surface)] px-2.5 py-2 font-sans text-sm text-tx-0';

/** One inhibit rule: which incidents hold back which others, and when two incidents count as related. */
export function InhibitRuleDrawer({
  open,
  editing,
  onClose,
}: {
  open: boolean;
  editing: InhibitRule | null;
  onClose: () => void;
}) {
  const { t } = useTranslation('alerts');
  const queryClient = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'alerts.inhibitions.manage' });
  const [input, setInput] = React.useState(newRuleInput);
  const [equalText, setEqualText] = React.useState('service');
  const [pairs, setPairs] = React.useState<inhibitionsApi.InhibitPair[] | null>(null);

  React.useEffect(() => {
    if (!open) return;

    const start = editing ? ruleInput(editing) : newRuleInput();

    setInput(start);
    setEqualText(start.equal.join(', '));
    setPairs(null);
  }, [open, editing]);

  const change = (next: Partial<typeof input>) => {
    setInput((current) => ({ ...current, ...next }));
    // What was previewed no longer says what this rule does.
    setPairs(null);
  };

  const problem = ruleProblem(input);

  const save = useMutation({
    mutationFn: () => {
      const payload = cleanInput(input);
      return editing ? inhibitionsApi.update(editing.id, payload) : inhibitionsApi.create(payload);
    },
    onSuccess: () => {
      toast.success(editing ? t('inhibitions.toast_updated') : t('inhibitions.toast_created'));
      void queryClient.invalidateQueries({ queryKey: INHIBIT_RULES_KEY });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const preview = useMutation({
    mutationFn: () => inhibitionsApi.preview(cleanInput(input)),
    onSuccess: setPairs,
    onError: (error) => toast.error(toApiError(error).message),
  });

  const severityOptions = [
    { value: '', label: t('inhibitions.fields.any_severity') },
    ...SEVERITIES.map((severity) => ({ value: severity, label: t(`severity.${severity}`) })),
  ];

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={editing ? t('inhibitions.edit_title') : t('inhibitions.drawer_title')}
      subtitle={t('inhibitions.drawer_subtitle')}
      width={760}
      footer={
        <FormSubmitFooter
          busy={save.isPending}
          disabled={manageAccess.disabled}
          disabledReason={manageAccess.reason}
          invalid={problem !== null}
          onCancel={onClose}
          submitLabel={editing ? t('inhibitions.save') : t('inhibitions.create')}
          formId="inhibit-rule-form"
        />
      }
    >
      <form
        id="inhibit-rule-form"
        data-inhibit-rule-form
        onSubmit={(event) => {
          event.preventDefault();

          if (!manageAccess.allowed) return;

          if (problem) {
            toast.error(t(`inhibitions.errors.${problem}`));
            return;
          }

          save.mutate();
        }}
      >
        <fieldset
          disabled={manageAccess.disabled}
          aria-disabled={manageAccess.disabled || undefined}
          className="contents"
        >
          <FormSection title={t('inhibitions.sections.identity')}>
            <FormField label={t('inhibitions.fields.name')} required>
              <FormInput
                value={input.name}
                onChange={(event) => change({ name: event.target.value })}
                placeholder={t('inhibitions.fields.name_placeholder')}
                required
              />
            </FormField>
            <label className={CHECK_ROW}>
              <Checkbox checked={input.enabled} onCheckedChange={(next) => change({ enabled: next === true })} />
              <span>{t('inhibitions.fields.enabled')}</span>
            </label>
          </FormSection>

          <FormSection title={t('inhibitions.sections.source')} description={t('inhibitions.sections.source_hint')}>
            <FormField label={t('inhibitions.fields.min_severity')}>
              <FormSelect
                value={input.source.minSeverity ?? ''}
                onChange={(value) => change({ source: { ...input.source, minSeverity: (value as Severity) || null } })}
                options={severityOptions}
              />
            </FormField>
            <MatchersEditor
              matchers={input.source.matchers}
              onChange={(matchers) => change({ source: { ...input.source, matchers } })}
            />
          </FormSection>

          <FormSection title={t('inhibitions.sections.target')} description={t('inhibitions.sections.target_hint')}>
            <FormField label={t('inhibitions.fields.max_severity')}>
              <FormSelect
                value={input.target.maxSeverity ?? ''}
                onChange={(value) => change({ target: { ...input.target, maxSeverity: (value as Severity) || null } })}
                options={severityOptions}
              />
            </FormField>
            <MatchersEditor
              matchers={input.target.matchers}
              onChange={(matchers) => change({ target: { ...input.target, matchers } })}
            />
          </FormSection>

          <FormSection title={t('inhibitions.sections.relation')}>
            <FormField label={t('inhibitions.fields.equal')} hint={t('inhibitions.fields.equal_hint')}>
              <FormInput
                className="font-mono"
                value={equalText}
                onChange={(event) => {
                  setEqualText(event.target.value);
                  change({ equal: equalFromText(event.target.value) });
                }}
                placeholder="service, env"
              />
            </FormField>
            <label className={CHECK_ROW}>
              <Checkbox
                checked={input.moreSevereOnly}
                onCheckedChange={(next) => change({ moreSevereOnly: next === true })}
              />
              <span>{t('inhibitions.fields.more_severe_only')}</span>
            </label>
            {problem === 'no_scope' && (
              <p role="alert" className="font-sans text-xs text-[var(--red-ink)]">
                {t('inhibitions.errors.no_scope')}
              </p>
            )}
          </FormSection>

          <FormSection title={t('inhibitions.sections.timing')}>
            <div className="grid grid-cols-2 gap-2">
              <FormField label={t('inhibitions.fields.wait_secs')} hint={t('inhibitions.fields.wait_hint')}>
                <FormInput
                  type="number"
                  min={0}
                  max={600}
                  value={input.waitSecs}
                  onChange={(event) => change({ waitSecs: Number(event.target.value) })}
                />
              </FormField>
              <FormField label={t('inhibitions.fields.linger_secs')} hint={t('inhibitions.fields.linger_hint')}>
                <FormInput
                  type="number"
                  min={0}
                  max={3600}
                  value={input.lingerSecs}
                  onChange={(event) => change({ lingerSecs: Number(event.target.value) })}
                />
              </FormField>
            </div>
          </FormSection>

          <FormSection title={t('inhibitions.sections.comment')}>
            <FormField label={t('inhibitions.fields.comment')}>
              <FormTextarea
                value={input.comment}
                onChange={(event) => change({ comment: event.target.value })}
                rows={2}
              />
            </FormField>
          </FormSection>

          <FormSection
            title={t('inhibitions.sections.preview')}
            description={t('inhibitions.sections.preview_hint')}
            className="mb-0"
          >
            <div>
              <ChromeButton
                type="button"
                size="sm"
                disabled={problem === 'no_scope' || preview.isPending}
                onClick={() => preview.mutate()}
                data-inhibit-preview-run
              >
                {t('inhibitions.preview.run')}
              </ChromeButton>
            </div>
            {pairs !== null && (
              <div data-inhibit-preview className="font-sans text-xs text-tx-2">
                {pairs.length === 0 ? (
                  <p>{t('inhibitions.preview.none')}</p>
                ) : (
                  <ul className="flex flex-col gap-1">
                    {pairs.map((pair) => (
                      <li key={pair.targetIncidentId} className="min-w-0 truncate">
                        <Link
                          className="text-tx-0 underline-offset-2 hover:underline"
                          to={`/alerts/incidents/${pair.targetIncidentId}`}
                        >
                          {pair.targetSummary}
                        </Link>{' '}
                        {t('inhibitions.preview.held_by')}{' '}
                        <Link
                          className="text-tx-0 underline-offset-2 hover:underline"
                          to={`/alerts/incidents/${pair.sourceIncidentId}`}
                        >
                          {pair.sourceSummary}
                        </Link>
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            )}
          </FormSection>
        </fieldset>
      </form>
    </FormDrawer>
  );
}
