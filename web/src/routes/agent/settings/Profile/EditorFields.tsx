import {
  Bot,
  Database,
  Gauge,
  GlobeLock,
  ShieldCheck,
  Wrench,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { RegisteredTool } from '@/api/agent';
import type { ModelProvider } from '@/api/agent/modelProviders';
import { uiLabelClass } from '@/shell/chrome';
import {
  type FormSelectOption,
  FormField,
  FormInput,
  FormRow,
  FormSection,
  FormSelect,
  FormTextarea,
} from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { Button } from '@/shell/ui/button';
import { Switch } from '@/shell/ui/switch';

import type { ProfileDraft, ProfileEditorPane } from './model';
import { ProfileTagInput } from './TagInput';
import { AgentToolChecklist } from '../../AgentToolChecklist';

const PANE_ICONS = {
  identity: Bot,
  tools: Wrench,
  data: Database,
  network: GlobeLock,
  limits: Gauge,
  approvals: ShieldCheck,
} as const;

export function ProfileEditorNavigation({
  panes,
  activePane,
  onChange,
}: {
  panes: readonly ProfileEditorPane[];
  activePane: ProfileEditorPane;
  onChange: (pane: ProfileEditorPane) => void;
}) {
  const { t } = useTranslation('agent');

  return (
    <nav
      aria-label={t('settings.profiles.navigation_label')}
      className="mb-6 flex flex-wrap gap-1 border-b border-bd-0 pb-3"
    >
      {panes.map((pane) => {
        const Icon = PANE_ICONS[pane];
        return (
          <button
            key={pane}
            type="button"
            aria-current={activePane === pane ? 'step' : undefined}
            onClick={() => onChange(pane)}
            className={cn(
              'inline-flex min-h-10 items-center gap-2 rounded-md px-3 text-sm font-strong transition-colors',
              activePane === pane
                ? 'bg-indigo-dim text-indigo'
                : 'text-tx-2 hover:bg-bg-2 hover:text-tx-0',
            )}
          >
            <Icon className="h-4 w-4" />
            {t(`settings.profiles.sections.${pane}`)}
          </button>
        );
      })}
    </nav>
  );
}

export function ProfileEditorFields({
  pane,
  draft,
  providers,
  tools,
  onChange,
}: {
  pane: ProfileEditorPane;
  draft: ProfileDraft;
  providers: ModelProvider[];
  tools: RegisteredTool[];
  onChange: (patch: Partial<ProfileDraft>) => void;
}) {
  const { t } = useTranslation('agent');

  if (pane === 'identity') {
    return (
      <FormSection title={t('settings.profiles.sections.identity')}>
        <FormField label={t('settings.profiles.fields.name')} required>
          <FormInput
            value={draft.name}
            onChange={(event) => onChange({ name: event.target.value })}
            placeholder={t('settings.profiles.name_placeholder')}
            required
            autoFocus
          />
        </FormField>
        <FormField label={t('settings.profiles.fields.description')}>
          <FormTextarea
            value={draft.description}
            onChange={(event) => onChange({ description: event.target.value })}
          />
        </FormField>
        <FormRow>
          <FormField label={t('settings.profiles.fields.provider')}>
            <FormSelect
              value={draft.providerId}
              onChange={(providerId) => onChange({ providerId })}
              options={profileModelProviderOptions(
                providers,
                t('settings.profiles.provider_auto'),
              )}
            />
          </FormField>
          <FormField label={t('settings.profiles.fields.model')}>
            <FormInput
              value={draft.model}
              onChange={(event) => onChange({ model: event.target.value })}
              placeholder={t('settings.profiles.model_inherit')}
            />
          </FormField>
        </FormRow>
        <div className="grid gap-x-5 sm:grid-cols-2">
          <SwitchRow
            label={t('settings.profiles.fields.enabled')}
            checked={draft.enabled}
            onCheckedChange={(enabled) => onChange({ enabled })}
          />
          <SwitchRow
            label={t('settings.profiles.fields.default')}
            checked={draft.isDefault}
            onCheckedChange={(isDefault) => onChange({ isDefault })}
          />
        </div>
      </FormSection>
    );
  }

  if (pane === 'tools') {
    return (
      <FormSection
        title={t('settings.profiles.sections.tools')}
        description={t('settings.profiles.sections.tools_description')}
      >
        <div className="flex min-h-10 flex-wrap items-center gap-2 border-y border-bd-0 py-2">
          <span className="min-w-0 flex-1 text-xs text-tx-2">
            {t('settings.profiles.tools_selected', {
              selected: draft.allowedTools.length,
              total: tools.length,
            })}
          </span>
          <Button
            type="button"
            size="sm"
            variant="ghost"
            onClick={() => onChange({ allowedTools: tools.map((tool) => tool.name) })}
          >
            {t('settings.profiles.select_all_tools')}
          </Button>
          <Button
            type="button"
            size="sm"
            variant="ghost"
            onClick={() => onChange({ allowedTools: [] })}
          >
            {t('settings.profiles.clear_tools')}
          </Button>
        </div>
        <AgentToolChecklist
          options={tools.map((tool) => ({
            value: tool.name,
            label: tool.display_name,
            hint: tool.description,
          }))}
          selected={draft.allowedTools}
          onChange={(allowedTools) => onChange({ allowedTools })}
        />
      </FormSection>
    );
  }

  if (pane === 'data') {
    return (
      <FormSection title={t('settings.profiles.sections.data')}>
        <ProfileTagField
          label={t('settings.data.environments')}
          values={draft.environments}
          onChange={(environments) => onChange({ environments })}
          placeholder={t('settings.profiles.tag_placeholder')}
          hint={t('settings.profiles.tag_hint')}
          removeLabel={(value) =>
            t('settings.profiles.remove_tag', { value })
          }
        />
        <ProfileTagField
          label={t('settings.data.services')}
          values={draft.services}
          onChange={(services) => onChange({ services })}
          placeholder={t('settings.profiles.tag_placeholder')}
          hint={t('settings.profiles.optional_tag_hint')}
          removeLabel={(value) =>
            t('settings.profiles.remove_tag', { value })
          }
        />
        <ProfileTagField
          label={t('settings.data.streams')}
          values={draft.streams}
          onChange={(streams) => onChange({ streams })}
          placeholder={t('settings.profiles.tag_placeholder')}
          hint={t('settings.profiles.optional_tag_hint')}
          removeLabel={(value) =>
            t('settings.profiles.remove_tag', { value })
          }
        />
      </FormSection>
    );
  }

  if (pane === 'network') {
    const allowed = draft.networkAccess === 'allowed';
    return (
      <FormSection
        title={t('settings.profiles.sections.network')}
        description={t('settings.network.editor_help')}
      >
        <SwitchRow
          label={t('settings.network.allow_network_access')}
          checked={allowed}
          onCheckedChange={(networkAllowed) =>
            onChange({ networkAccess: networkAllowed ? 'allowed' : 'blocked' })
          }
        />
        <p
          className={cn(
            'border-l-2 pl-3 text-sm leading-6',
            allowed
              ? 'border-yellow/60 text-yellow-soft'
              : 'border-green/60 text-green-soft',
          )}
        >
          {t(
            allowed
              ? 'settings.network.allowed_explanation'
              : 'settings.network.blocked_explanation',
          )}
        </p>
      </FormSection>
    );
  }

  if (pane === 'limits') {
    return (
      <FormSection title={t('settings.profiles.sections.limits')}>
        <FormRow>
          <FormField label={t('settings.profiles.fields.context_tokens')} required>
            <FormInput
              type="number"
              min={1}
              value={draft.maxContextTokens}
              onChange={(event) => onChange({ maxContextTokens: event.target.value })}
              required
            />
          </FormField>
          <FormField
            label={t('settings.profiles.fields.duration_minutes')}
            required
          >
            <FormInput
              type="number"
              min={1}
              value={draft.maxInvestigationMinutes}
              onChange={(event) =>
                onChange({ maxInvestigationMinutes: event.target.value })
              }
              required
            />
          </FormField>
        </FormRow>
        <FormField label={t('settings.profiles.fields.tool_calls')} required>
          <FormInput
            type="number"
            min={1}
            max={256}
            value={draft.maxToolCalls}
            onChange={(event) => onChange({ maxToolCalls: event.target.value })}
            required
          />
        </FormField>
      </FormSection>
    );
  }

  const riskOptions = [
    { value: 'automatic', label: t('settings.profiles.policies.automatic') },
    { value: 'approval', label: t('settings.profiles.policies.approval') },
    {
      value: 'two_person_approval',
      label: t('settings.profiles.policies.two_person_approval'),
    },
  ];
  return (
    <FormSection title={t('settings.profiles.sections.approvals')}>
      {(['l0', 'l1', 'l2', 'l3'] as const).map((risk) => (
        <FormField
          key={risk}
          label={`${risk.toUpperCase()} · ${t(`settings.approval_policy.${risk}`)}`}
        >
          <FormSelect
            value={draft[`${risk}Policy`]}
            onChange={(policy) => onChange({ [`${risk}Policy`]: policy })}
            options={riskOptions}
          />
        </FormField>
      ))}
    </FormSection>
  );
}

function SwitchRow({
  label,
  checked,
  onCheckedChange,
}: {
  label: string;
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
}) {
  return (
    <div className="flex min-h-11 items-center gap-3 py-2">
      <span className="min-w-0 flex-1 text-sm font-strong text-tx-1">{label}</span>
      <Switch
        checked={checked}
        onCheckedChange={onCheckedChange}
        aria-label={label}
      />
    </div>
  );
}

export function profileModelProviderOptions(
  providers: readonly ModelProvider[],
  automaticLabel: string,
): FormSelectOption[] {
  return [
    { value: '', label: automaticLabel },
    ...providers.map((provider) => ({
      value: provider.id,
      label: provider.name,
    })),
  ];
}

function ProfileTagField({
  label,
  values,
  onChange,
  placeholder,
  hint,
  removeLabel,
}: {
  label: string;
  values: string[];
  onChange: (values: string[]) => void;
  placeholder: string;
  hint: string;
  removeLabel: (value: string) => string;
}) {
  const inputId = React.useId();
  const hintId = `${inputId}-hint`;
  return (
    <div className="flex flex-col gap-1.5">
      <label htmlFor={inputId} className={uiLabelClass}>
        {label}
      </label>
      <ProfileTagInput
        id={inputId}
        describedBy={hintId}
        values={values}
        onChange={onChange}
        placeholder={placeholder}
        removeLabel={removeLabel}
      />
      <span id={hintId} className="text-xs leading-relaxed text-tx-3">
        {hint}
      </span>
    </div>
  );
}
