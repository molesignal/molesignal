import { useQuery } from '@tanstack/react-query';
import { Braces, FileText, GitBranch } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { NotifyCategory } from '@/api/notify';
import * as templatesApi from '@/api/notify/templates';

import { FoldSection } from './FoldSection';
import { isPlainText, type TemplateChannel } from './model';

/** Conditions ready to insert: the template keeps what is inside only when they hold. */
const CONDITION_SNIPPETS = [
  { key: 'triggered', token: '{{#if event.type == "alert.triggered"}}…{{/if}}' },
  { key: 'resolved', token: '{{#if event.type == "alert.resolved"}}…{{/if}}' },
  { key: 'severity', token: '{{#if severity == "critical"}}…{{else}}…{{/if}}' },
  { key: 'runbook', token: '{{#if annotations.runbook_url}}{{annotations.runbook_url}}{{/if}}' },
] as const;

type ReferenceTabKey =
  | 'common'
  | 'rule'
  | 'incident'
  | 'escalation'
  | 'metadata'
  | 'schedule'
  | 'oncall'
  | 'override';

interface ReferenceTab {
  key: ReferenceTabKey;
  groups: templatesApi.NotifyTemplateFieldGroup[];
}

function referenceTabs(category: NotifyCategory): ReferenceTab[] {
  const common: ReferenceTab = {
    key: 'common',
    groups: ['event', 'message'],
  };
  if (category === 'alert' || category === 'escalation') {
    return [
      common,
      { key: 'rule', groups: ['rule'] },
      { key: 'incident', groups: ['incident', 'trigger'] },
      ...(category === 'escalation'
        ? [{ key: 'escalation' as const, groups: ['escalation' as const] }]
        : []),
      { key: 'metadata', groups: ['labels', 'annotations'] },
    ];
  }
  if (category === 'oncall') {
    return [
      common,
      { key: 'schedule', groups: ['schedule'] },
      { key: 'oncall', groups: ['oncall'] },
      { key: 'override', groups: ['override'] },
    ];
  }
  return [common];
}

/** The presets a template on `channel` is offered: an SMS or phone template only those written for
 * its channel, others the general ones and those written for their channel, these first. */
function presetsFor(
  presets: templatesApi.NotifyTemplatePreset[],
  category: NotifyCategory,
  channel: TemplateChannel,
): templatesApi.NotifyTemplatePreset[] {
  return presets.filter(
    (preset) =>
      preset.category === category &&
      (preset.channel === channel || (preset.channel === null && !isPlainText(channel))),
  );
}

function TokenButton({
  token,
  description,
  example,
  onInsert,
}: {
  token: string;
  description?: string;
  example?: string;
  onInsert: (token: string) => void;
}) {
  return (
    <button
      type="button"
      onClick={() => onInsert(token)}
      aria-label={`${token}${description ? ` · ${description}` : ''}`}
      title={[description, example].filter(Boolean).join(' · ')}
      className="flex min-h-10 min-w-0 items-center justify-between gap-3 rounded-md px-3 py-2 text-left transition-colors hover:bg-bg-1 focus-visible:bg-bg-3"
    >
      <code className="min-w-0 break-all rounded border border-bd-0 bg-bg-1 px-1.5 py-0.5 font-mono text-xs text-indigo-soft">
        {token}
      </code>
      {description && (
        <span className="max-w-[46%] shrink-0 text-right text-xs leading-4 text-tx-2">
          {description}
        </span>
      )}
    </button>
  );
}

/** The variables a template can use and the presets it can start from, each folded until opened. */
export function TemplateReferencePanel({
  category,
  channel,
  onInsert,
  onUsePreset,
}: {
  category: NotifyCategory;
  channel: TemplateChannel;
  onInsert: (token: string) => void;
  onUsePreset: (preset: templatesApi.NotifyTemplatePreset) => void;
}) {
  const { t, i18n } = useTranslation('notify');
  const tabs = React.useMemo(() => referenceTabs(category), [category]);
  const [activeTabKey, setActiveTabKey] =
    React.useState<ReferenceTabKey>('common');
  // The presets come in the reader's interface language.
  const catalog = useQuery({
    queryKey: ['notify', 'template-fields', i18n.language],
    queryFn: templatesApi.listFields,
  });
  const activeTab =
    tabs.find((tab) => tab.key === activeTabKey) ?? tabs[0]!;
  const visibleFields = (catalog.data?.fields ?? []).filter(
    (field) =>
      field.categories.includes(category) &&
      activeTab.groups.includes(field.group),
  );
  const dynamicFields =
    activeTab.key === 'metadata'
      ? [
          ...(catalog.data?.labelKeys ?? []).map((key) => ({
            key: `labels.${key}`,
            token: `{{labels.${key}}}`,
          })),
          ...(catalog.data?.annotationKeys ?? []).map((key) => ({
            key: `annotations.${key}`,
            token: `{{annotations.${key}}}`,
          })),
        ]
      : [];
  const presets = presetsFor(catalog.data?.presets ?? [], category, channel);

  return (
    <div className="space-y-4">
      <FoldSection
        icon={Braces}
        title={t('templates.reference.title')}
        description={t('templates.reference.description')}
      >
        <div
          role="tablist"
          aria-label={t('templates.reference.title')}
          className="flex gap-1 overflow-x-auto border-b border-bd-0 p-1.5"
        >
          {tabs.map((tab) => {
            const selected = tab.key === activeTab.key;
            return (
              <button
                key={tab.key}
                type="button"
                role="tab"
                aria-selected={selected}
                onClick={() => setActiveTabKey(tab.key)}
                className={`min-h-9 shrink-0 rounded-md px-3 text-xs font-semibold transition-colors ${
                  selected
                    ? 'bg-indigo-dim text-indigo-soft'
                    : 'text-tx-2 hover:bg-bg-1 hover:text-tx-0 focus-visible:bg-bg-3'
                }`}
              >
                {t(`templates.reference.tabs.${tab.key}`)}
              </button>
            );
          })}
        </div>
        {catalog.isLoading ? (
          <p className="px-3 py-4 text-xs text-tx-3">
            {t('templates.reference.loading')}
          </p>
        ) : catalog.isError ? (
          <p className="px-3 py-4 text-xs text-red-soft">
            {t('templates.reference.error')}
          </p>
        ) : (
          <div className="max-h-72 overflow-y-auto p-2">
            <div className="grid grid-cols-1 gap-0.5">
              {visibleFields.map((field) => (
                <TokenButton
                  key={field.key}
                  token={field.token}
                  description={t(
                    `templates.reference.fields.${field.key}`,
                  )}
                  example={field.example}
                  onInsert={onInsert}
                />
              ))}
              {dynamicFields.map((field) => (
                <TokenButton
                  key={field.key}
                  token={field.token}
                  description={t('templates.reference.dynamic_key')}
                  onInsert={onInsert}
                />
              ))}
            </div>
          </div>
        )}
      </FoldSection>

      <FoldSection
        icon={GitBranch}
        title={t('templates.conditions.title')}
        description={t('templates.conditions.description')}
      >
        <div className="grid grid-cols-1 gap-0.5 p-2">
          {CONDITION_SNIPPETS.map((snippet) => (
            <TokenButton
              key={snippet.key}
              token={snippet.token}
              description={t(`templates.conditions.items.${snippet.key}`)}
              onInsert={onInsert}
            />
          ))}
        </div>
      </FoldSection>

      {presets.length > 0 && (
        <FoldSection
          icon={FileText}
          title={t('templates.presets.title')}
          description={t('templates.presets.description')}
        >
          <div className="grid grid-cols-1 gap-1 p-2 sm:grid-cols-2">
            {presets.map((preset) => (
              <button
                key={preset.key}
                type="button"
                onClick={() => onUsePreset(preset)}
                title={preset.body}
                className="flex min-h-11 items-center justify-between gap-2 rounded-md px-3 text-left transition-colors hover:bg-bg-1 focus-visible:bg-bg-3"
              >
                <span className="text-xs font-semibold text-tx-0">
                  {t(`templates.presets.items.${preset.key}`)}
                </span>
                <span className="shrink-0 font-mono text-xs text-tx-3">
                  {preset.format}
                </span>
              </button>
            ))}
          </div>
        </FoldSection>
      )}
    </div>
  );
}

export function useTemplateTokenInserter(
  ref: React.RefObject<HTMLTextAreaElement | null>,
  setValue: React.Dispatch<React.SetStateAction<string>>,
) {
  return React.useCallback(
    (token: string) => {
      const input = ref.current;
      if (!input) {
        setValue((current) => `${current}${token}`);
        return;
      }
      const start = input.selectionStart ?? input.value.length;
      const end = input.selectionEnd ?? input.value.length;
      setValue((current) => `${current.slice(0, start)}${token}${current.slice(end)}`);
      window.requestAnimationFrame(() => {
        input.focus();
        input.setSelectionRange(start + token.length, start + token.length);
      });
    },
    [ref, setValue],
  );
}
