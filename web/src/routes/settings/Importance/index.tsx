import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { ArrowDown, ArrowUp, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { PageHeader } from '@/admin';
import * as importanceApi from '@/api/importance';
import type { ImportanceLevels, UrgencyLevel } from '@/api/importance';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton, IconButton } from '@/shell/chrome';
import { FormInput, FormSelect } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';

import {
  BUILT_IN_URGENCIES,
  MAX_PRIORITY_LEVELS,
  MAX_URGENCY_LEVELS,
  SEVERITIES,
  giveSeverity,
  levelsProblem,
  moveUrgency,
  newUrgency,
  withPriorityCount,
} from './model';
import { SectionBody, SettingsGroupStack, SettingsSection } from '../_atoms';

const QUERY_KEY = ['alerts', 'importance'] as const;

/**
 * The priority levels and urgency levels of the organization: how many priorities and their
 * names, and what each urgency does when it pages.
 */
export function ImportanceSettings() {
  const { t } = useTranslation('settings-admin');
  const queryClient = useQueryClient();
  const access = useActionAccess({ permission: 'alerts.importance.manage' });
  const query = useQuery({ queryKey: QUERY_KEY, queryFn: importanceApi.get });
  const [draft, setDraft] = React.useState<ImportanceLevels | null>(null);

  React.useEffect(() => {
    if (query.data && draft === null) setDraft(query.data.levels);
  }, [draft, query.data]);

  const save = useMutation({
    mutationFn: (levels: ImportanceLevels) => importanceApi.save(levels),
    onSuccess: (saved) => {
      queryClient.setQueryData(QUERY_KEY, saved);
      setDraft(saved.levels);
      toast.success(t('importance.saved'));
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const problem = draft ? levelsProblem(draft) : null;
  const dirty = Boolean(draft && query.data && JSON.stringify(draft) !== JSON.stringify(query.data.levels));
  const disabled = access.disabled || !draft || save.isPending;

  return (
    <>
      <PageHeader
        title={t('importance.title')}
        subtitle={t('importance.subtitle') as string}
        actions={
          <ChromeButton
            variant="primary"
            disabled={disabled || !dirty || problem !== null}
            disabledReason={access.reason ?? (problem ? t(`importance.errors.${problem}`) : undefined)}
            onClick={() => draft && save.mutate(draft)}
            data-testid="importance-save"
          >
            {t('importance.save')}
          </ChromeButton>
        }
      />
      <SectionBody opensOnSection className="pb-10">
        {draft && (
          <SettingsGroupStack>
            <PrioritySection levels={draft} onChange={setDraft} disabled={disabled} />
            <UrgencySection levels={draft} onChange={setDraft} disabled={disabled} />
            {problem && (
              <p className="text-sm text-[var(--red-ink)]" data-importance-problem={problem}>
                {t(`importance.errors.${problem}`)}
              </p>
            )}
          </SettingsGroupStack>
        )}
      </SectionBody>
    </>
  );
}

function PrioritySection({
  levels,
  onChange,
  disabled,
}: {
  levels: ImportanceLevels;
  onChange: (levels: ImportanceLevels) => void;
  disabled: boolean;
}) {
  const { t } = useTranslation('settings-admin');
  const count = levels.priorities.length;

  return (
    <SettingsSection title={t('importance.priorities.title')} description={t('importance.priorities.description')}>
      <ul className="flex flex-col gap-2" data-priority-levels={count}>
        {levels.priorities.map((level, index) => (
          <li key={level.priority} className="grid grid-cols-[48px_minmax(0,200px)_minmax(0,1fr)] items-center gap-2">
            <span className="font-strong tabular-nums text-tx-0">{level.priority}</span>
            <FormInput
              aria-label={t('importance.priorities.name', { priority: level.priority })}
              placeholder={level.priority}
              value={level.name}
              disabled={disabled}
              onChange={(event) =>
                onChange({
                  ...levels,
                  priorities: levels.priorities.map((item, at) => (at === index ? { ...item, name: event.target.value } : item)),
                })
              }
            />
            <FormInput
              aria-label={t('importance.priorities.description_of', { priority: level.priority })}
              placeholder={t('importance.priorities.description_placeholder')}
              value={level.description}
              disabled={disabled}
              onChange={(event) =>
                onChange({
                  ...levels,
                  priorities: levels.priorities.map((item, at) =>
                    at === index ? { ...item, description: event.target.value } : item,
                  ),
                })
              }
            />
          </li>
        ))}
      </ul>
      <div className="mt-3 flex gap-2">
        <ChromeButton
          disabled={disabled || count >= MAX_PRIORITY_LEVELS}
          onClick={() => onChange(withPriorityCount(levels, count + 1))}
          data-testid="priority-add"
        >
          {t('importance.priorities.add')}
        </ChromeButton>
        <ChromeButton
          disabled={disabled || count <= 1}
          onClick={() => onChange(withPriorityCount(levels, count - 1))}
          data-testid="priority-remove"
        >
          {t('importance.priorities.remove', { priority: `P${count}` })}
        </ChromeButton>
      </div>
    </SettingsSection>
  );
}

function UrgencySection({
  levels,
  onChange,
  disabled,
}: {
  levels: ImportanceLevels;
  onChange: (levels: ImportanceLevels) => void;
  disabled: boolean;
}) {
  const { t } = useTranslation('settings-admin');
  const { t: ta } = useTranslation('alerts');
  const [adding, setAdding] = React.useState('');
  const change = (index: number, next: Partial<UrgencyLevel>) =>
    onChange({
      ...levels,
      urgencies: levels.urgencies.map((level, at) => (at === index ? { ...level, ...next } : level)),
    });

  return (
    <SettingsSection title={t('importance.urgencies.title')} description={t('importance.urgencies.description')}>
      <ol className="flex flex-col gap-3" data-urgency-levels={levels.urgencies.length}>
        {levels.urgencies.map((level, index) => {
          const builtIn = BUILT_IN_URGENCIES.includes(level.key);

          return (
            <li key={level.key || index} className="flex flex-col gap-2 rounded-md bg-bg-2 p-3" data-urgency-level={level.key}>
              <div className="flex flex-wrap items-center gap-2">
                <code className="min-w-[72px] font-mono text-xs text-tx-1">{level.key}</code>
                <FormInput
                  aria-label={t('importance.urgencies.name', { key: level.key })}
                  placeholder={ta(`importance.urgency.${level.key}`, { defaultValue: level.key })}
                  value={level.name}
                  disabled={disabled}
                  className="max-w-[200px]"
                  onChange={(event) => change(index, { name: event.target.value })}
                />
                <span className="flex-1" />
                <IconButton
                  aria-label={t('importance.urgencies.up', { key: level.key })}
                  disabled={disabled || index === 0}
                  onClick={() => onChange(moveUrgency(levels, index, -1))}
                >
                  <ArrowUp className="h-3.5 w-3.5" />
                </IconButton>
                <IconButton
                  aria-label={t('importance.urgencies.down', { key: level.key })}
                  disabled={disabled || index === levels.urgencies.length - 1}
                  onClick={() => onChange(moveUrgency(levels, index, 1))}
                >
                  <ArrowDown className="h-3.5 w-3.5" />
                </IconButton>
                <IconButton
                  aria-label={t('importance.urgencies.remove', { key: level.key })}
                  disabled={disabled || builtIn}
                  disabledReason={builtIn ? t('importance.urgencies.built_in') : undefined}
                  className="enabled:hover:text-red-soft"
                  onClick={() => onChange({ ...levels, urgencies: levels.urgencies.filter((_, at) => at !== index) })}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </IconButton>
              </div>
              <div className="grid gap-2 sm:grid-cols-3">
                <label className="flex min-h-9 items-center justify-between gap-3 rounded-md bg-[var(--control-surface)] px-2.5 text-sm text-tx-0">
                  <span>{t('importance.urgencies.escalates')}</span>
                  <Switch
                    checked={level.escalates}
                    disabled={disabled}
                    onCheckedChange={(escalates) => change(index, { escalates })}
                    aria-label={t('importance.urgencies.escalates')}
                  />
                </label>
                <FormSelect
                  ariaLabel={t('importance.urgencies.quiet_hours')}
                  value={level.quietHours}
                  disabled={disabled}
                  onChange={(quiet) => change(index, { quietHours: quiet as UrgencyLevel['quietHours'] })}
                  options={[
                    { value: 'block', label: t('importance.urgencies.quiet.block') },
                    { value: 'switch', label: t('importance.urgencies.quiet.switch') },
                  ]}
                />
                <FormSelect
                  ariaLabel={t('importance.urgencies.steps')}
                  value={level.steps}
                  disabled={disabled}
                  onChange={(steps) => change(index, { steps: steps as UrgencyLevel['steps'] })}
                  options={[
                    { value: 'high', label: t('importance.urgencies.step_groups.high') },
                    { value: 'low', label: t('importance.urgencies.step_groups.low') },
                  ]}
                />
              </div>
              <div role="group" aria-label={t('importance.urgencies.severities')} className="flex flex-wrap items-center gap-1">
                <span className="mr-1 text-xs text-tx-2">{t('importance.urgencies.severities')}</span>
                {SEVERITIES.map((severity) => {
                  const on = level.severities.includes(severity);

                  return (
                    <button
                      key={severity}
                      type="button"
                      aria-pressed={on}
                      disabled={disabled}
                      onClick={() => onChange(giveSeverity(levels, level.key, severity))}
                      className={cn(
                        'rounded-md px-2.5 py-1 text-xs transition-colors',
                        on ? 'bg-indigo-dim font-strong text-indigo-soft' : 'bg-[var(--control-surface)] text-tx-2 hover:bg-bg-3',
                      )}
                    >
                      {ta(`history.severity.${severity}`)}
                    </button>
                  );
                })}
              </div>
            </li>
          );
        })}
      </ol>
      <div className="mt-3 flex items-center gap-2">
        <FormInput
          aria-label={t('importance.urgencies.new_key')}
          placeholder="medium"
          value={adding}
          disabled={disabled || levels.urgencies.length >= MAX_URGENCY_LEVELS}
          className="max-w-[200px]"
          onChange={(event) => setAdding(event.target.value.trim().toLowerCase())}
        />
        <ChromeButton
          disabled={disabled || levels.urgencies.length >= MAX_URGENCY_LEVELS || !/^[a-z][a-z0-9_]{0,31}$/.test(adding)}
          data-testid="urgency-add"
          onClick={() => {
            const lowest = levels.urgencies.findIndex((level) => level.key === 'low');
            const urgencies = [...levels.urgencies];
            urgencies.splice(lowest < 0 ? urgencies.length : lowest, 0, newUrgency(adding));
            onChange({ ...levels, urgencies });
            setAdding('');
          }}
        >
          {t('importance.urgencies.add')}
        </ChromeButton>
      </div>
    </SettingsSection>
  );
}
