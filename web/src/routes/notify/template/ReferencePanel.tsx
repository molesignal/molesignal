import { useQuery } from '@tanstack/react-query';
import { Braces, FileText } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { NotifyCategory } from '@/api/notify';
import * as templatesApi from '@/api/notify/templates';

type ReferenceTabKey =
  | 'common'
  | 'rule'
  | 'incident'
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

export function TemplateReferencePanel({
  category,
  onInsert,
  onUsePreset,
}: {
  category: NotifyCategory;
  onInsert: (token: string) => void;
  onUsePreset: (preset: templatesApi.NotifyTemplatePreset) => void;
}) {
  const { t } = useTranslation('notify');
  const tabs = React.useMemo(() => referenceTabs(category), [category]);
  const [activeTabKey, setActiveTabKey] =
    React.useState<ReferenceTabKey>('common');
  const catalog = useQuery({
    queryKey: ['notify', 'template-fields'],
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
          ...(catalog.data?.label_keys ?? []).map((key) => ({
            key: `labels.${key}`,
            token: `{{labels.${key}}}`,
          })),
          ...(catalog.data?.annotation_keys ?? []).map((key) => ({
            key: `annotations.${key}`,
            token: `{{annotations.${key}}}`,
          })),
        ]
      : [];
  const presets = (catalog.data?.presets ?? []).filter(
    (preset) => preset.category === category,
  );

  return (
    <div className="space-y-4">
      <section className="rounded-md border border-bd-0 bg-bg-2">
        <div className="flex min-h-11 items-center gap-2 border-b border-bd-0 px-3">
          <Braces className="h-4 w-4 text-tx-2" />
          <div>
            <h3 className="text-xs font-semibold text-tx-0">
              {t('templates.reference.title')}
            </h3>
            <p className="text-xs text-tx-3">
              {t('templates.reference.description')}
            </p>
          </div>
        </div>
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
      </section>

      {presets.length > 0 && (
        <section className="rounded-md border border-bd-0 bg-bg-2">
          <div className="flex min-h-11 items-center gap-2 border-b border-bd-0 px-3">
            <FileText className="h-4 w-4 text-tx-2" />
            <div>
              <h3 className="text-xs font-semibold text-tx-0">
                {t('templates.presets.title')}
              </h3>
              <p className="text-xs text-tx-3">
                {t('templates.presets.description')}
              </p>
            </div>
          </div>
          <div className="grid grid-cols-1 gap-1 p-2 sm:grid-cols-3">
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
        </section>
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
